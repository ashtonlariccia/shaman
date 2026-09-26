/**
 * The frontend half of the performance harness (see `crates/shaman-app/src/bench.rs`).
 *
 * Runs in a real tab, through the real input and output paths, and times three
 * points for everything it sends:
 *
 * - **arrived**: the first output chunk reaches the webview. Covers the IPC
 *   write, ConPTY, the shell, the pump's batching and the IPC channel back.
 * - **parsed**: xterm has consumed it (an empty `write` queued behind it
 *   calls back once everything ahead of it is parsed).
 * - **painted**: the next animation frame after that. xterm's renderer
 *   schedules its own frame while parsing, so by ours it has drawn.
 *
 * Only the pipeline Shaman controls is in these numbers. The compositor, the
 * display and the keyboard add their own time on top, in every terminal alike.
 */
import type { Terminal } from "@xterm/xterm";
import { Channel, invoke } from "@tauri-apps/api/core";

export type BenchConfig = {
  profileId: string;
  fileBytes: number;
  marker: string;
  /** Typed into the shell to print the file. */
  command: string;
};

export type BenchContext = {
  config: BenchConfig;
  term: Terminal;
  /** The same path a keystroke takes to the shell. */
  send: (data: string) => void;
  /** Called with every output chunk as it arrives, before xterm sees it. */
  tap: (fn: ((bytes: Uint8Array) => void) | null) => void;
  conptyAlone: () => Promise<{ conpty: string; ms: number; wireBytes: number }>;
  /** One empty invoke, for the fixed cost of crossing into Rust and back. */
  ping: () => Promise<unknown>;
};

const ECHO_TRIALS = 60;

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const frame = () => new Promise<number>((r) => requestAnimationFrame(() => r(performance.now())));
const parsed = (t: Terminal) => new Promise<number>((r) => t.write("", () => r(performance.now())));

function stats(xs: number[]) {
  const s = [...xs].sort((a, b) => a - b);
  const at = (q: number) => s[Math.min(s.length - 1, Math.floor(q * s.length))];
  const round = (x: number) => Math.round(x * 100) / 100;
  return { p50: round(at(0.5)), p95: round(at(0.95)), max: round(s[s.length - 1]) };
}

/** Resolve on the next chunk; the timestamp is taken as it lands. */
function nextChunk(ctx: BenchContext): Promise<number> {
  return new Promise((resolve) => {
    ctx.tap(() => {
      ctx.tap(null);
      resolve(performance.now());
    });
  });
}

/** Wait until the shell has been quiet for `ms` -- the prompt is up. */
async function settle(ctx: BenchContext, ms: number) {
  let last = performance.now();
  ctx.tap(() => (last = performance.now()));
  while (performance.now() - last < ms) await sleep(50);
  ctx.tap(null);
}

async function echoLatency(ctx: BenchContext) {
  const arrived: number[] = [];
  const parsedAt: number[] = [];
  const painted: number[] = [];

  for (let i = 0; i < ECHO_TRIALS; i++) {
    const got = nextChunk(ctx);
    const t0 = performance.now();
    ctx.send("x");
    const t1 = await got;
    const t2 = await parsed(ctx.term);
    const t3 = await frame();
    arrived.push(t1 - t0);
    parsedAt.push(t2 - t0);
    painted.push(t3 - t0);

    const erased = nextChunk(ctx);
    ctx.send("\b");
    await erased;
    // Off the frame grid, so trials do not all start at the same phase.
    await sleep(20 + Math.random() * 30);
  }
  return { arrived: stats(arrived), parsed: stats(parsedAt), painted: stats(painted) };
}

/** Frame intervals while `during` runs, for how smooth a flood looks. */
async function watchFrames<T>(during: Promise<T>) {
  const gaps: number[] = [];
  let running = true;
  let last = performance.now();
  const loop = (now: number) => {
    gaps.push(now - last);
    last = now;
    if (running) requestAnimationFrame(loop);
  };
  requestAnimationFrame(loop);
  const result = await during;
  running = false;
  return { result, frames: stats(gaps), frameCount: gaps.length };
}

async function throughput(ctx: BenchContext) {
  const { config, term } = ctx;
  const marker = new TextEncoder().encode(config.marker);
  const chunks: Uint8Array[] = [];
  let wireBytes = 0;
  let tail = new Uint8Array(0);

  const run = new Promise<number>((resolve) => {
    ctx.tap((bytes) => {
      chunks.push(bytes.slice());
      wireBytes += bytes.length;
      const joined = new Uint8Array(tail.length + bytes.length);
      joined.set(tail);
      joined.set(bytes, tail.length);
      if (indexOf(joined, marker) >= 0) {
        ctx.tap(null);
        resolve(performance.now());
      }
      tail = joined.slice(Math.max(0, joined.length - marker.length));
    });
  });

  const t0 = performance.now();
  ctx.send(`${config.command}\r`);
  const watched = await watchFrames(
    run.then(async (lastByte) => ({ lastByte, parsedAt: await parsed(term) })),
  );
  const painted = await frame();
  const { lastByte, parsedAt } = watched.result;

  const mbps = (ms: number) => Math.round((config.fileBytes / 1048576 / (ms / 1000)) * 10) / 10;
  return {
    live: {
      wireBytes,
      msLastByte: Math.round(lastByte - t0),
      msParsed: Math.round(parsedAt - t0),
      msPainted: Math.round(painted - t0),
      fileMBps: mbps(painted - t0),
      frames: watched.frames,
      frameCount: watched.frameCount,
    },
    chunks,
  };
}

/**
 * The bench file streamed from Rust through the real pump and channel, with no
 * shell or ConPTY producing it. Against `live` it says whether a flood's cost
 * is the IPC path or the programs on the other end of the PTY.
 */
async function ipcFlood(ctx: BenchContext) {
  const { term, config } = ctx;
  term.reset();
  await parsed(term);
  let received = 0;
  const t0 = performance.now();
  const done = new Promise<number>((resolve) => {
    const channel = new Channel<ArrayBuffer | number[]>();
    channel.onmessage = (message) => {
      const bytes =
        message instanceof ArrayBuffer
          ? new Uint8Array(message)
          : Uint8Array.from(message as number[]);
      term.write(bytes);
      received += bytes.length;
      if (received >= config.fileBytes) resolve(performance.now());
    };
    void invoke("bench_ipc_flood", { onOutput: channel });
  });
  const watched = await watchFrames(
    done.then(async (lastByte) => ({ lastByte, parsedAt: await parsed(term) })),
  );
  const painted = await frame();
  const ms = painted - t0;
  return {
    msLastByte: Math.round(watched.result.lastByte - t0),
    msPainted: Math.round(ms),
    fileMBps: Math.round((config.fileBytes / 1048576 / (ms / 1000)) * 10) / 10,
    frames: watched.frames,
  };
}

/** xterm alone: the captured stream written straight back in. */
async function replay(ctx: BenchContext, chunks: Uint8Array[]) {
  const { term } = ctx;
  term.reset();
  await parsed(term);
  const bytes = chunks.reduce((n, c) => n + c.length, 0);
  const t0 = performance.now();
  const watched = await watchFrames(
    (async () => {
      for (const c of chunks) term.write(c);
      return parsed(term);
    })(),
  );
  const painted = await frame();
  const ms = painted - t0;
  return {
    wireBytes: bytes,
    msPainted: Math.round(ms),
    wireMBps: Math.round((bytes / 1048576 / (ms / 1000)) * 10) / 10,
    frames: watched.frames,
  };
}

function indexOf(hay: Uint8Array, needle: Uint8Array): number {
  outer: for (let i = 0; i + needle.length <= hay.length; i++) {
    for (let j = 0; j < needle.length; j++) if (hay[i + j] !== needle[j]) continue outer;
    return i;
  }
  return -1;
}

async function ipcRoundTrip(ctx: BenchContext) {
  const times: number[] = [];
  for (let i = 0; i < 60; i++) {
    const t0 = performance.now();
    await ctx.ping();
    times.push(performance.now() - t0);
  }
  return stats(times);
}

export async function runBench(ctx: BenchContext): Promise<Record<string, unknown>> {
  const { term } = ctx;
  await settle(ctx, 1000);

  const ipcMs = await ipcRoundTrip(ctx);

  const conptyAlone = await ctx.conptyAlone();
  await settle(ctx, 300);

  const echo = await echoLatency(ctx);
  await settle(ctx, 300);

  const { live, chunks } = await throughput(ctx);
  await settle(ctx, 500);

  const xtermAlone = await replay(ctx, chunks);
  await settle(ctx, 300);

  const ipcAlone = await ipcFlood(ctx);

  const renderer = term.element?.querySelector("canvas") ? "webgl" : "dom";
  const mem = (performance as unknown as { memory?: { usedJSHeapSize: number } }).memory;

  return {
    conpty: conptyAlone.conpty,
    renderer,
    size: { cols: term.cols, rows: term.rows },
    fileBytes: ctx.config.fileBytes,
    conptyAlone: {
      ms: Math.round(conptyAlone.ms),
      wireBytes: conptyAlone.wireBytes,
      fileMBps:
        Math.round((ctx.config.fileBytes / 1048576 / (conptyAlone.ms / 1000)) * 10) / 10,
    },
    ipcMs,
    echoMs: echo,
    throughput: live,
    xtermAlone,
    ipcAlone,
    jsHeapMB: mem ? Math.round(mem.usedJSHeapSize / 1048576) : null,
  };
}
