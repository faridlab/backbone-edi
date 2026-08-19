-- Revert the ADR-0014 strict fence re-statement for edi module.
-- The fence predates this migration (ADR-0008-era), so the honest reverse is to
-- re-state the same live policy, not to disarm the tables: a down that disabled RLS
-- would leave company data unfenced — a posture this module never had.

-- Re-state the pre-existing fence for edi.edi_documents (identical policy; see header).
DROP POLICY IF EXISTS edi_documents_company_isolation ON edi.edi_documents;
CREATE POLICY edi_documents_company_isolation ON edi.edi_documents
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- Re-state the pre-existing fence for edi.trading_partners (identical policy; see header).
DROP POLICY IF EXISTS trading_partners_company_isolation ON edi.trading_partners;
CREATE POLICY trading_partners_company_isolation ON edi.trading_partners
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

