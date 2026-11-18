// The only storage the core needs. src/worker.ts adapts D1 to it; the tests adapt bun:sqlite.
// Statements are single SQLite statements with `?` placeholders. Anything that must happen at
// most once (accepting an invite, using a join code) is a single conditional UPDATE whose
// `changes` says whether this call won, so no transactions are needed.

export type Param = string | number | null;

export interface Db {
  run(sql: string, ...params: Param[]): Promise<{ changes: number }>;
  get<T>(sql: string, ...params: Param[]): Promise<T | null>;
  all<T>(sql: string, ...params: Param[]): Promise<T[]>;
}
