-- Hand-authored (user-owned). Not regenerated.
--
-- Best-effort restore sketch for the tenancy strip (ADR-0029). This is a breaking module
-- release against dev-stage databases: the down re-adds the company_id column as nullable
-- with its plain indexes and the company isolation policy shape, but restores NO data —
-- rows written after the strip (or after the decorator re-keyed them) carry org_unit_id
-- only. The composing service's tenancy decorator remains the live fence; treat this
-- down as a schema-shape sketch for archaeology, not a usable rollback.
--
-- The outbox_events / inbox_consumed relay tables are untouched by the up and by this down.

ALTER TABLE edi.edi_documents   ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE edi.trading_partners ADD COLUMN IF NOT EXISTS company_id uuid;

CREATE INDEX IF NOT EXISTS idx_edi_documents_company_id_status
    ON edi.edi_documents (company_id, status);
CREATE UNIQUE INDEX IF NOT EXISTS idx_trading_partners_company_id_partner_code
    ON edi.trading_partners (company_id, partner_code);

-- The company isolation policies (ADR-0008 shape) reference company_id, so they are
-- re-created AFTER the column exists again.
ALTER TABLE edi.edi_documents ENABLE ROW LEVEL SECURITY;
ALTER TABLE edi.edi_documents FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS edi_documents_company_isolation ON edi.edi_documents;
CREATE POLICY edi_documents_company_isolation ON edi.edi_documents
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

ALTER TABLE edi.trading_partners ENABLE ROW LEVEL SECURITY;
ALTER TABLE edi.trading_partners FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS trading_partners_company_isolation ON edi.trading_partners;
CREATE POLICY trading_partners_company_isolation ON edi.trading_partners
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
