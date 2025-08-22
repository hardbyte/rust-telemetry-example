//! Real tokio task metrics collection from console subscriber.
//!
//! This module connects to the tokio console subscriber to extract actual
//! task information and export it as OpenTelemetry metrics.

use console_api::instrument::{instrument_client::InstrumentClient, InstrumentRequest};
use opentelemetry::metrics::{Counter, Gauge, Meter, MeterProvider};
use opentelemetry::KeyValue;
use std::collections::HashMap;
use std::time::Duration;
use tokio::task::JoinHandle;
use tonic::transport::Channel;
use tracing::{debug, error, info, warn};

/// Real tokio task metrics collector that extracts data from console subscriber.
#[derive(Debug)]
pub struct TokioTaskMetrics {
    service_name: String,
    console_port: u16,
    _meter: Meter,
    task_states: Gauge<u64>,
    task_total: Counter<u64>,
    task_by_location: Gauge<u64>,
}

impl TokioTaskMetrics {
    /// Creates a new real tokio task metrics collector.
    pub fn new(
        service_name: impl Into<String>,
        console_port: u16,
        meter_provider: &dyn MeterProvider,
    ) -> Self {
        let service_name = service_name.into();
        let meter = meter_provider.meter("tokio_console_metrics");

        let task_states = meter
            .u64_gauge("tokio.console.task.states")
            .with_description("Number of tasks by state and location")
            .build();

        let task_total = meter
            .u64_counter("tokio.console.task.total")
            .with_description("Total tasks by spawn location")
            .build();

        let task_by_location = meter
            .u64_gauge("tokio.console.task.by_location")
            .with_description("Active tasks grouped by spawn location")
            .build();

        Self {
            service_name,
            console_port,
            _meter: meter,
            task_states,
            task_total,
            task_by_location,
        }
    }

    /// Starts collecting real tokio task data from console subscriber.
    pub fn start_collection(self, interval: Duration) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            interval_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            info!(
                "Starting real tokio task data collection from console subscriber on port {} every {:?}",
                self.console_port, interval
            );

            loop {
                interval_timer.tick().await;
                
                if let Err(e) = self.collect_console_data().await {
                    debug!("Console data collection error (this is normal if console subscriber isn't ready): {}", e);
                }
            }
        })
    }

    /// Connects to console subscriber and extracts task data.
    async fn collect_console_data(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let console_addr = format!("http://127.0.0.1:{}", self.console_port);
        
        // Try to connect with short timeout
        let channel = tonic::transport::Endpoint::from_shared(console_addr)?
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(3))
            .connect()
            .await?;
            
        let mut client = InstrumentClient::new(channel);
        
        // Get task details
        match self.fetch_task_details(&mut client).await {
            Ok(_) => {
                info!(
                    service = %self.service_name,
                    "Successfully collected tokio task data from console subscriber"
                );
            }
            Err(e) => {
                debug!("Failed to fetch task details: {}", e);
            }
        }
        
        Ok(())
    }

    /// Fetches detailed task information from the console API.
    async fn fetch_task_details(&self, client: &mut InstrumentClient<Channel>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Use watch_updates to get general runtime updates including task information
        let request = tonic::Request::new(InstrumentRequest {});
        
        let mut stream = client.watch_updates(request).await?.into_inner();
        
        // Get one update with timeout
        if let Some(update) = tokio::time::timeout(Duration::from_secs(1), stream.message()).await?? {
            self.process_runtime_update(update).await?;
        }
        
        Ok(())
    }

    /// Process a runtime update from the console subscriber.
    async fn process_runtime_update(&self, update: console_api::instrument::Update) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut location_counts = HashMap::new();
        let mut state_counts = HashMap::new();
        
        // Extract task data from the update
        if let Some(task_update) = update.task_update {
            info!("Processing {} new tasks from console subscriber", task_update.new_tasks.len());
            
            for task in task_update.new_tasks {
                // Extract task information
                let location = self.extract_task_location(&task);
                let state = self.extract_task_state(&task);
                
                // Count by location
                *location_counts.entry(location.clone()).or_insert(0u64) += 1;
                
                // Count by state  
                let state_key = format!("{}::{}", location, state);
                *state_counts.entry(state_key).or_insert(0u64) += 1;
                
                // Record task creation
                let labels = &[
                    KeyValue::new("service.name", self.service_name.clone()),
                    KeyValue::new("task.location", location),
                ];
                self.task_total.add(1, labels);
            }
        } else {
            info!("No task update in console subscriber message");
        }
        
        // Record current counts as gauge metrics
        for (location, count) in &location_counts {
            let labels = &[
                KeyValue::new("service.name", self.service_name.clone()),
                KeyValue::new("task.location", location.clone()),
            ];
            self.task_by_location.record(*count, labels);
        }
        
        for (state_key, count) in &state_counts {
            let parts: Vec<&str> = state_key.splitn(2, "::").collect();
            if parts.len() == 2 {
                let labels = &[
                    KeyValue::new("service.name", self.service_name.clone()),
                    KeyValue::new("task.location", parts[0].to_string()),
                    KeyValue::new("task.state", parts[1].to_string()),
                ];
                self.task_states.record(*count, labels);
            }
        }
        
        // Note: TaskDetails doesn't have stats_update field like TaskUpdate would
        
        info!(
            service = %self.service_name,
            unique_locations = location_counts.len(),
            "Processed tokio console task data"
        );
        
        Ok(())
    }

    /// Extract a human-readable task location from task data.
    fn extract_task_location(&self, task: &console_api::tasks::Task) -> String {
        if let Some(location) = &task.location {
            // Try to get a meaningful location string
            if let Some(file) = &location.file {
                let filename = file.split('/').last().unwrap_or(file);
                format!("{}:{}", filename, location.line.unwrap_or(0))
            } else {
                "unknown_location".to_string()
            }
        } else {
            "no_location".to_string()
        }
    }

    /// Extract task state information.
    fn extract_task_state(&self, task: &console_api::tasks::Task) -> String {
        // Look at the task's current state
        match task.kind {
            0 => "spawned".to_string(),     // Spawn
            1 => "local".to_string(),       // Local  
            _ => format!("unknown({})", task.kind),
        }
    }
}