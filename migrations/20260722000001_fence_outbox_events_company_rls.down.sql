DROP POLICY IF EXISTS outbox_events_company_isolation ON edi.outbox_events;
ALTER TABLE edi.outbox_events NO FORCE ROW LEVEL SECURITY;
ALTER TABLE edi.outbox_events DISABLE ROW LEVEL SECURITY;
DROP INDEX IF EXISTS edi.idx_edi_outbox_company_id;
ALTER TABLE edi.outbox_events DROP COLUMN IF EXISTS company_id;
