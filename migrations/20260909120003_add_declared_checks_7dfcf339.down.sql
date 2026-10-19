-- Down: drop the declared rule constraints for edi module
ALTER TABLE IF EXISTS edi.trading_partners DROP CONSTRAINT IF EXISTS ck_trading_partners_partner_code_max_length__max_length;
ALTER TABLE IF EXISTS edi.trading_partners DROP CONSTRAINT IF EXISTS ck_trading_partners_partner_code_required__required;
ALTER TABLE IF EXISTS edi.trading_partners DROP CONSTRAINT IF EXISTS ck_trading_partners_name_max_length__max_length;
ALTER TABLE IF EXISTS edi.trading_partners DROP CONSTRAINT IF EXISTS ck_trading_partners_name_required__required;
ALTER TABLE IF EXISTS edi.edi_documents DROP CONSTRAINT IF EXISTS ck_edi_documents_payload_max_length__max_length;
ALTER TABLE IF EXISTS edi.edi_documents DROP CONSTRAINT IF EXISTS ck_edi_documents_payload_required__required;
ALTER TABLE IF EXISTS edi.edi_documents DROP CONSTRAINT IF EXISTS ck_edi_documents_business_key_max_length__max_length;
ALTER TABLE IF EXISTS edi.edi_documents DROP CONSTRAINT IF EXISTS ck_edi_documents_business_key_required__required;
ALTER TABLE IF EXISTS edi.edi_documents DROP CONSTRAINT IF EXISTS ck_edi_documents_control_number_max_length__max_length;
ALTER TABLE IF EXISTS edi.edi_documents DROP CONSTRAINT IF EXISTS ck_edi_documents_control_number_required__required;
ALTER TABLE IF EXISTS edi.edi_documents DROP CONSTRAINT IF EXISTS ck_edi_documents_partner_id_required__required;
DO $$
BEGIN
    IF to_regclass('edi.uq_edi_documents_business_key__unique') IS NOT NULL AND to_regclass('edi.idx_edi_documents_partner_id_direction_business_key') IS NULL THEN
        ALTER INDEX edi.uq_edi_documents_business_key__unique RENAME TO idx_edi_documents_partner_id_direction_business_key;
    END IF;
END $$;

