-- Down: restore the is_active boolean exactly as it was.
-- Only 'inactive' rows are written back as FALSE; rows at the column default
-- map to the boolean default TRUE without an UPDATE.

ALTER TABLE edi.trading_partners ADD COLUMN is_active BOOLEAN NOT NULL DEFAULT TRUE;
UPDATE edi.trading_partners SET is_active = FALSE WHERE status = 'inactive';
ALTER TABLE edi.trading_partners DROP COLUMN status;

DROP TYPE IF EXISTS trading_partner_status;
