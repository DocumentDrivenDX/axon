/**
 * Tests for the local read replica (FEAT-032).
 *
 * The replica is transport-agnostic, so these tests drive it with a fake
 * in-memory ChangeSource — no running server required.
 */

import { describe, it, expect } from "vitest";
import {
  LocalReplica,
  asyncIterableSource,
  type ChangeEvent,
} from "../src/local-replica.js";

// Helper: build a fake change source from a static list of events.
function fakeSource(events: ChangeEvent[]) {
  return asyncIterableSource(events);
}

// A snapshot bootstrap (op "r") followed by live deltas, with a tombstone.
function scenario(): ChangeEvent[] {
  return [
    // --- snapshot (op "r") ---
    {
      op: "r",
      collection: "tasks",
      id: "t-001",
      after: { title: "alpha", status: "open", priority: 2 },
      cursor: "tok-1",
    },
    {
      op: "r",
      collection: "tasks",
      id: "t-002",
      after: { title: "bravo", status: "done", priority: 5 },
      cursor: "tok-2",
    },
    {
      op: "r",
      collection: "tasks",
      id: "t-003",
      after: { title: "charlie", status: "open", priority: 1 },
      cursor: "tok-3",
    },
    // --- live deltas ---
    // create a new entity
    {
      op: "c",
      collection: "tasks",
      id: "t-004",
      after: { title: "delta", status: "open", priority: 9 },
      cursor: "tok-4",
    },
    // update an existing entity
    {
      op: "u",
      collection: "tasks",
      id: "t-002",
      after: { title: "bravo", status: "open", priority: 5 },
      cursor: "tok-5",
    },
    // delete (tombstone) an entity
    {
      op: "d",
      collection: "tasks",
      id: "t-001",
      cursor: "tok-6",
    },
  ];
}

describe("LocalReplica", () => {
  it("applies a snapshot then deltas: upserts and tombstone removals", async () => {
    const replica = new LocalReplica();
    await replica.consume(fakeSource(scenario()));

    // t-001 was deleted -> tombstoned -> gone.
    expect(replica.get("tasks", "t-001")).toBeUndefined();

    // t-002 was updated (status open).
    expect(replica.get("tasks", "t-002")?.data).toEqual({
      title: "bravo",
      status: "open",
      priority: 5,
    });

    // t-003 untouched from snapshot.
    expect(replica.get("tasks", "t-003")?.data.title).toBe("charlie");

    // t-004 created live.
    expect(replica.get("tasks", "t-004")?.data.priority).toBe(9);

    // 3 remaining records (t-002, t-003, t-004).
    expect(replica.size).toBe(3);
  });

  it("query with equality filter returns matching records", async () => {
    const replica = new LocalReplica();
    await replica.consume(fakeSource(scenario()));

    const open = replica.query("tasks", { filter: { status: "open" } });
    const ids = open.map((r) => r.id).sort();
    // t-002 (updated to open), t-003, t-004 are open; t-001 deleted.
    expect(ids).toEqual(["t-002", "t-003", "t-004"]);
  });

  it("query with sort returns ordered results (asc and desc)", async () => {
    const replica = new LocalReplica();
    await replica.consume(fakeSource(scenario()));

    const asc = replica.query("tasks", {
      filter: { status: "open" },
      sort: { field: "priority", dir: "asc" },
    });
    expect(asc.map((r) => r.data.priority)).toEqual([1, 5, 9]);

    const desc = replica.query("tasks", {
      filter: { status: "open" },
      sort: { field: "priority", dir: "desc" },
    });
    expect(desc.map((r) => r.data.priority)).toEqual([9, 5, 1]);

    // default sort dir is asc.
    const def = replica.query("tasks", {
      filter: { status: "open" },
      sort: { field: "priority" },
    });
    expect(def.map((r) => r.data.priority)).toEqual([1, 5, 9]);
  });

  it("query scopes results to the requested collection", async () => {
    const replica = new LocalReplica();
    await replica.consume(
      fakeSource([
        { op: "r", collection: "tasks", id: "t-1", after: { x: 1 }, cursor: "a" },
        { op: "r", collection: "users", id: "u-1", after: { x: 2 }, cursor: "b" },
      ]),
    );

    expect(replica.query("tasks").map((r) => r.id)).toEqual(["t-1"]);
    expect(replica.query("users").map((r) => r.id)).toEqual(["u-1"]);
  });

  it("tracks the latest applied opaque cursor token for resume", async () => {
    const replica = new LocalReplica();
    expect(replica.cursor).toBeUndefined();

    await replica.consume(fakeSource(scenario()));
    // Last event applied was the delete with cursor "tok-6".
    expect(replica.cursor).toBe("tok-6");
  });

  it("resumes by consuming a second source without re-bootstrapping", async () => {
    const replica = new LocalReplica();
    await replica.consume(fakeSource(scenario()));
    expect(replica.cursor).toBe("tok-6");

    // Simulate reconnect: a new live delta arrives after the resume point.
    await replica.consume(
      fakeSource([
        {
          op: "u",
          collection: "tasks",
          id: "t-003",
          after: { title: "charlie", status: "done", priority: 1 },
          cursor: "tok-7",
        },
      ]),
    );

    expect(replica.cursor).toBe("tok-7");
    expect(replica.get("tasks", "t-003")?.data.status).toBe("done");
    // Existing records survived the "reconnect".
    expect(replica.size).toBe(3);
  });

  it("a tombstone for an entity not in the store is a no-op", async () => {
    const replica = new LocalReplica();
    await replica.consume(
      fakeSource([
        { op: "d", collection: "tasks", id: "ghost", cursor: "z-1" },
      ]),
    );
    expect(replica.get("tasks", "ghost")).toBeUndefined();
    expect(replica.size).toBe(0);
    // Cursor still advances even when the delete matched nothing.
    expect(replica.cursor).toBe("z-1");
  });

  it("apply can be driven event-by-event (no source required)", () => {
    const replica = new LocalReplica();
    replica.apply({
      op: "c",
      collection: "tasks",
      id: "t-1",
      after: { title: "x" },
      cursor: "c-1",
    });
    expect(replica.get("tasks", "t-1")?.data.title).toBe("x");
    expect(replica.cursor).toBe("c-1");
  });
});
