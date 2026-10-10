-- clean_db.sql
-- One-shot maintenance script for SQLite database to clean legacy contaminated dimensions
-- and debug extreme values on mma-stats production server.
--
-- Usage:
--   sqlite3 telemetry.db < clean_db.sql

BEGIN TRANSACTION;

-- 1. Purge unauthorized user-selected algorithm rows from daily_agg
DELETE FROM daily_agg
WHERE dim = 'alg'
  AND val NOT IN ('Mixed', 'Sunny', 'Azusa', 'Daniel', 'Roxy', 'Companella');

-- 2. Merge historical composite capsule 'Azusa+Companella' into canonical 'Azusa'
INSERT INTO daily_agg (day, dim, val, count, sum_val, max_val, min_val)
SELECT day, 'actual', 'Azusa', count, sum_val, max_val, min_val
FROM daily_agg
WHERE dim = 'actual' AND val = 'Azusa+Companella'
ON CONFLICT(day, dim, val) DO UPDATE SET
  count = count + excluded.count,
  sum_val = sum_val + excluded.sum_val,
  max_val = MAX(max_val, excluded.max_val),
  min_val = MIN(min_val, excluded.min_val);

DELETE FROM daily_agg
WHERE dim = 'actual' AND val = 'Azusa+Companella';

-- 3. Purge unauthorized actual algorithm rows from daily_agg
DELETE FROM daily_agg
WHERE dim = 'actual'
  AND val NOT IN ('Sunny', 'Daniel', 'Azusa', 'Roxy', 'Companella');

-- 4. Purge duration rows with debug anomaly values (> 30s)
DELETE FROM daily_agg
WHERE dim = 'dur' AND max_val > 30000;

-- 5. Purge installs records with unofficial or unknown versions
DELETE FROM installs
WHERE version = ''
   OR version = 'unknown'
   OR version LIKE '%dev%'
   OR version LIKE '%test%'
   OR version LIKE '%dirty%'
   OR version LIKE '%debug%'
   OR version LIKE '%local%';

COMMIT;

VACUUM;
