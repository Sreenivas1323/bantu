// Bantu Connect on Cloudflare Workers with D1. All the logic is in src/core; this file only
// adapts D1 to the core's Db interface. See docs/CONNECT.md.

import { handle } from "./core/app.ts";
import type { Secrets } from "./core/context.ts";
import type { Db, Param } from "./core/db.ts";

export type Env = Secrets & { DB: D1Database };

export class D1Db implements Db {
  constructor(private readonly d1: D1Database) {}

  async run(sql: string, ...params: Param[]): Promise<{ changes: number }> {
    const res = await this.d1.prepare(sql).bind(...params).run();
    return { changes: res.meta.changes ?? 0 };
  }

  async get<T>(sql: string, ...params: Param[]): Promise<T | null> {
    return (await this.d1.prepare(sql).bind(...params).first<T>()) ?? null;
  }

  async all<T>(sql: string, ...params: Param[]): Promise<T[]> {
    return (await this.d1.prepare(sql).bind(...params).all<T>()).results;
  }
}

export default {
  fetch(request, env) {
    return handle(request, {
      db: new D1Db(env.DB),
      env,
      fetch: (input, init) => fetch(input, init),
      now: () => Date.now(),
    });
  },
} satisfies ExportedHandler<Env>;
