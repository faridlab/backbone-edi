-- Down: remove the company RLS fence for edi module

-- Reverse the company RLS fence for edi.edi_documents
DROP POLICY IF EXISTS edi_documents_company_isolation ON edi.edi_documents;
ALTER TABLE edi.edi_documents NO FORCE ROW LEVEL SECURITY;
ALTER TABLE edi.edi_documents DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for edi.trading_partners
DROP POLICY IF EXISTS trading_partners_company_isolation ON edi.trading_partners;
ALTER TABLE edi.trading_partners NO FORCE ROW LEVEL SECURITY;
ALTER TABLE edi.trading_partners DISABLE ROW LEVEL SECURITY;

