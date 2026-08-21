-- Migration: replace the trading-partner lifecycle boolean with a status enum
-- edi.trading_partners carried `is_active BOOLEAN NOT NULL DEFAULT TRUE`; the tree-wide convention
-- is one `status` enum field per lifecycle (see docs/refactoring-schema in the serpa workspace).
-- The boolean migrates only rows deviating from its own column default. The enum type is created
-- unqualified so it lands beside the module's other enum types (public), where the generated
-- sqlx type_name resolves.

DO $$ BEGIN
    CREATE TYPE trading_partner_status AS ENUM ('active', 'inactive');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

ALTER TABLE edi.trading_partners ADD COLUMN status trading_partner_status NOT NULL DEFAULT 'active';
UPDATE edi.trading_partners SET status = 'inactive' WHERE NOT is_active;
ALTER TABLE edi.trading_partners DROP COLUMN is_active;
