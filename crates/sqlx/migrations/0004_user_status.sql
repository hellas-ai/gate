-- Add disabled_at field to users table
ALTER TABLE users ADD COLUMN disabled_at TIMESTAMP;

-- Index for filtering by disabled status
CREATE INDEX IF NOT EXISTS idx_users_disabled_at ON users(disabled_at);