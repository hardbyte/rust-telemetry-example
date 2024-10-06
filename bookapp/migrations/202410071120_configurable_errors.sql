
CREATE TABLE error_injection_config (
                                        id SERIAL PRIMARY KEY,
                                        endpoint_pattern TEXT NOT NULL,
                                        http_method TEXT NOT NULL,
                                        error_rate DOUBLE PRECISION NOT NULL CHECK (error_rate >= 0.0 AND error_rate <= 1.0),
                                        error_code INTEGER NOT NULL CHECK (error_code >= 400 AND error_code <= 599),
                                        error_message TEXT,
                                        created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
                                        UNIQUE (endpoint_pattern, http_method)
);