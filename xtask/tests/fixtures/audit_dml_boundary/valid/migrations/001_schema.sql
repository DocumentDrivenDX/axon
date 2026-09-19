CREATE TABLE audit_log (id INTEGER PRIMARY KEY);
CREATE TRIGGER enforce_entity_write BEFORE INSERT ON entities FOR EACH ROW EXECUTE FUNCTION audit_entity_mutation();
CREATE FUNCTION audit_entity_mutation() LANGUAGE js AS "console.log();";
CREATE PROCEDURE refresh_entity_projection() LANGUAGE SQL AS BEGIN SELECT 1; END;
