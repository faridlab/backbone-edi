-- Hand-authored (user-owned). Not regenerated.
--
-- Strip every company-fence artifact from the edi tables (ADR-0029): the module is
-- tenant-agnostic; org scoping is installed by the COMPOSING service's tenancy decorator,
-- never by the module. Dropped here, per table: the company-leading indexes, the
-- <table>_company_isolation RLS policy, and the company_id column itself.
--
-- Ordering guard (the decorator must run FIRST on any database with data): the module
-- never moves tenancy data. A table is safe to strip when EITHER
--   a) it carries org_unit_id with no NULLs — the decorator backfilled it from company_id —
--      or b) it is empty (a fresh database: the earlier chain files created it empty).
-- Otherwise the strip RAISEs, naming the decorator step, rather than dropping a column
-- that still holds the only tenancy key. The file is re-runnable (every drop is IF EXISTS
-- and the tracker has no checksums), so a failed run retries cleanly after the decorator
-- lands.
--
-- RLS enable/force flags are deliberately NOT touched: the decorator owns those now.
--
-- The schema's outbox_events / inbox_consumed tables are framework relay infrastructure,
-- not schema models: their company axis and fence stay, by design.

DO $$
DECLARE
    t text;
    has_org boolean;
    org_nulls bigint;
    total bigint;
    offenders text := '';
BEGIN
    FOREACH t IN ARRAY ARRAY['edi_documents', 'trading_partners']
    LOOP
        IF to_regclass(format('edi.%I', t)) IS NULL THEN
            CONTINUE; -- chain not fully applied on this database; nothing to strip
        END IF;

        SELECT EXISTS (
                   SELECT 1 FROM information_schema.columns
                   WHERE table_schema = 'edi' AND table_name = t AND column_name = 'org_unit_id'
               )
        INTO has_org;

        EXECUTE format('SELECT count(*) FROM edi.%I', t) INTO total;

        IF has_org THEN
            EXECUTE format(
                'SELECT count(*) FROM edi.%I WHERE org_unit_id IS NULL', t)
            INTO org_nulls;
        ELSE
            org_nulls := total; -- no org column: every row's only tenancy key is company_id
        END IF;

        IF has_org AND org_nulls = 0 THEN
            CONTINUE; -- decorator backfilled: safe
        END IF;
        IF total = 0 THEN
            CONTINUE; -- empty table (fresh database): safe
        END IF;
        offenders := offenders || format(' edi.%s (%s rows, %s rows not covered by org_unit_id);', t, total, org_nulls);
    END LOOP;

    IF offenders <> '' THEN
        RAISE EXCEPTION 'refusing to strip company_id — these tables are not yet covered by the tenancy decorator:%. Apply the composing service''s tenancy decorator (it backfills org_unit_id from company_id) and re-run; it is the only step that moves tenancy data.', offenders;
    END IF;
END $$;

-- ── edi_documents ─────────────────────────────────────────────────────────────
DROP INDEX IF EXISTS edi.idx_edi_documents_company_id_status;
DROP POLICY IF EXISTS edi_documents_company_isolation ON edi.edi_documents;
ALTER TABLE edi.edi_documents DROP COLUMN IF EXISTS company_id;

-- ── trading_partners ──────────────────────────────────────────────────────────
DROP INDEX IF EXISTS edi.idx_trading_partners_company_id_partner_code;
DROP POLICY IF EXISTS trading_partners_company_isolation ON edi.trading_partners;
ALTER TABLE edi.trading_partners DROP COLUMN IF EXISTS company_id;

-- The company-free domain uniques are NOT touched: idx_edi_documents_partner_id_direction_business_key
-- carries no tenant column and stays module-owned. The per-unit trading-partner-code unique
-- (formerly [company_id, partner_code]) is POSTURE and is owned by the composing service's
-- tenancy decorator — it is intentionally NOT restored here (the pre-fence global form would
-- forbid two units of one tenant sharing a code).
