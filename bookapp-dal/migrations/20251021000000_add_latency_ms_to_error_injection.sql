-- Add latency_ms column to error_injection_config table
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name='error_injection_config' AND column_name='latency_ms'
    ) THEN
        ALTER TABLE error_injection_config
        ADD COLUMN latency_ms INTEGER CHECK (latency_ms IS NULL OR latency_ms >= 0);
    END IF;
END $$;
