-- Down: drop edi.trading_partners table
DROP TABLE IF EXISTS edi.trading_partners CASCADE;
DROP FUNCTION IF EXISTS edi.trading_partners_audit_timestamp() CASCADE;
