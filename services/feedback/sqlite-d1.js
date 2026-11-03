// The part of Cloudflare D1's API the Worker uses, over Deno's SQLite, for the tests and the
// local server. D1 is SQLite, so the Worker's SQL runs here unchanged.
import { DatabaseSync } from "node:sqlite";

export function memoryDatabase(schema, path = ":memory:") {
  const sqlite = new DatabaseSync(path);
  sqlite.exec(schema);
  const prepare = (sql) => {
    const statement = (values) => ({
      bind: (...next) => statement(next),
      first: () => Promise.resolve(sqlite.prepare(sql).get(...values) ?? null),
      all: () => Promise.resolve({ results: sqlite.prepare(sql).all(...values) }),
      run: () => Promise.resolve(sqlite.prepare(sql).run(...values)),
    });
    return statement([]);
  };
  return {
    prepare,
    batch: async (statements) => {
      sqlite.exec("BEGIN");
      try {
        for (const statement of statements) await statement.run();
        sqlite.exec("COMMIT");
      } catch (error) {
        sqlite.exec("ROLLBACK");
        throw error;
      }
    },
    exec: (sql) => sqlite.exec(sql),
    dump: () => Object.fromEntries(
      ["notes", "reports", "counters", "blocked"]
        .map((table) => [table, sqlite.prepare(`SELECT * FROM ${table}`).all()]),
    ),
  };
}
