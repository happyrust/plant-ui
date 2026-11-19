#!/usr/bin/env node
/**
 * 接口层回放（计划 model-load-speed-parity.md T0）：不经客户端，用 Node 直接问后端要记录，
 * 给出「不经客户端最快多快」的下限——用来把「服务端」与「客户端」各占多少分开。
 *
 * 三种取法，各跑 N 次报中位数、请求数、字节数（与 plant-ui 服务供数的三档取法一一对应）：
 *   whole    整根翻页：对节点自己一发 `model/records`，每页 5000 条按游标翻完（改前 plant-ui 的取法）
 *   members  按成员 6 路：`tree/children` 拿 ZONE，6 个成员同时在途、各自单根翻页（T1）
 *   batch    批量形：成员多于 64 个且后端声明了 `capabilities.model_records_batch_max_roots` 时几个成员
 *            打成一发（头几发 1、1、2、4、8，之后 16），一页装不下的按条数重新分组再发（T2）
 * 节点不是 SITE（`tree/children` 的成员不全是 ZONE）或成员为空时，members / batch 不适用，只报 whole。
 *
 * 只发只读请求：`GET /api/v1/health`、`GET /api/v1/tree/children`、`POST /api/v1/model/records`。
 * 不存响应体；不把任何地址写死——后端地址必须从参数或环境变量给。
 *
 * 用法（PowerShell / bash 同）：
 *   node scripts/model-load-replay.mjs --backend=http://127.0.0.1:8022 --project=<项目> --mdb=/<MDB> --namespace=<ns> \
 *        --refno=23717_2 [--repeat=3] [--strategies=whole,members,batch] [--concurrency=6] [--batch=16] [--page=5000] \
 *        [--jsonl=<追加写的 .jsonl>]
 *   后端地址也可用环境变量 PLANT_UI_REPLAY_BACKEND 给；参数优先。
 *
 * 参照：plant3d-web `.scratch/model-load-speed/api-replay.mjs`（取法与常量）、gen-model `docs/specs/web-service-api.md` §4.5.2（契约）。
 */
import { appendFileSync } from 'node:fs';

const args = Object.fromEntries(
  process.argv.slice(2).map((arg) => {
    const m = /^--([^=]+)(?:=(.*))?$/.exec(arg);
    return m ? [m[1], m[2] ?? 'true'] : [arg, 'true'];
  }),
);

function need(name, envName) {
  const value = args[name] ?? (envName ? process.env[envName] : undefined);
  if (!value || value === 'true') {
    console.error(`缺少 --${name}${envName ? `（或环境变量 ${envName}）` : ''}`);
    console.error('用法：node scripts/model-load-replay.mjs --backend=<地址> --project=<项目> --mdb=/<MDB> --namespace=<ns> --refno=<a_b> [--repeat=3]');
    process.exit(2);
  }
  return value;
}

const BACKEND = need('backend', 'PLANT_UI_REPLAY_BACKEND').replace(/\/+$/, '');
const PROJECT = need('project');
const MDB = need('mdb');
const NAMESPACE = need('namespace');
const KEY = String(need('refno')).replace('/', '_');
const REPEAT = Math.max(1, Number(args.repeat ?? 3) || 3);
const CONCURRENCY = Math.max(1, Number(args.concurrency ?? 6) || 6);
const PAGE = Math.min(5000, Math.max(1, Number(args.page ?? 5000) || 5000));
const WANT_BATCH = Math.max(1, Number(args.batch ?? 16) || 16);
const STRATEGIES = String(args.strategies ?? 'whole,members,batch').split(',').map((s) => s.trim()).filter(Boolean);
const JSONL = args.jsonl && args.jsonl !== 'true' ? args.jsonl : null;

/** 与 plant-ui / plant3d-web 相同的常量（计划 Q11）。 */
const RAMP = [1, 1, 2, 4, 8];
const MIN_MEMBERS_TO_BATCH = 64;
const MAX_ROOTS = 64;

const v1 = (k) => String(k).replace('_', '/');
const round = (v) => Math.round(v * 10) / 10;
const median = (xs) => {
  const s = [...xs].sort((a, b) => a - b);
  const mid = Math.floor(s.length / 2);
  return s.length % 2 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
};

function parseServerTiming(header) {
  const out = {};
  for (const part of String(header || '').split(',')) {
    const name = part.trim().split(';')[0];
    const dur = /dur=([0-9.]+)/.exec(part);
    if (name && dur) out[name] = Number(dur[1]);
  }
  return out;
}

async function getJson(path) {
  const resp = await fetch(`${BACKEND}${path}`);
  const text = await resp.text();
  if (!resp.ok) throw new Error(`GET ${path} → HTTP ${resp.status}: ${text.slice(0, 200)}`);
  return JSON.parse(text);
}

/** 一发 `model/records`。只量不存：回执解出来用完就丢。 */
async function postRecords(body) {
  const t0 = performance.now();
  const resp = await fetch(`${BACKEND}/api/v1/model/records`, {
    method: 'POST',
    headers: { 'content-type': 'application/json; charset=utf-8', accept: '*/*' },
    body: JSON.stringify({ project: PROJECT, mdb: MDB, namespace: NAMESPACE, ...body }),
  });
  const tHeaders = performance.now();
  const text = await resp.text();
  const tBody = performance.now();
  const timing = parseServerTiming(resp.headers.get('server-timing'));
  const json = resp.ok ? JSON.parse(text) : null;
  return {
    ok: resp.ok,
    status: resp.status,
    json,
    bytes: text.length,
    waitMs: tHeaders - t0,
    dlMs: tBody - tHeaders,
    serverRead: timing.read ?? 0,
    serverTotal: timing.total ?? 0,
    error: resp.ok ? null : text.slice(0, 200),
  };
}

function newAcc() {
  return { requests: 0, bytes: 0, items: 0, errors: 0, firstMs: null, sumWaitMs: 0, sumDlMs: 0, serverReadMs: 0, serverTotalMs: 0, regrouped: 0, pages: 0, errorText: null, t0: performance.now() };
}

function note(acc, r) {
  acc.requests++;
  acc.bytes += r.bytes;
  acc.sumWaitMs += r.waitMs;
  acc.sumDlMs += r.dlMs;
  acc.serverReadMs += r.serverRead;
  acc.serverTotalMs += r.serverTotal;
  if (acc.firstMs === null) acc.firstMs = performance.now() - acc.t0;
  if (!r.ok) {
    acc.errors++;
    acc.errorText ??= `HTTP ${r.status}: ${r.error}`;
  }
}

/** 单根翻页：改前 plant-ui 的取法（与 `model_update_api::model_records` 同一请求体）。 */
async function readSingle(acc, member) {
  let cursor = null;
  for (let page = 0; page < 10_000; page++) {
    const r = await postRecords({ generation_root: v1(member), limit: PAGE, cursor });
    note(acc, r);
    acc.pages++;
    if (!r.ok) return;
    acc.items += r.json.items.length;
    if (!r.json.truncated) return;
    if (r.json.next_cursor === null || r.json.next_cursor === undefined) {
      acc.errors++;
      acc.errorText ??= `${member}: truncated 却没有 next_cursor`;
      return;
    }
    cursor = r.json.next_cursor;
  }
}

/** 批量形一页切回各成员（与 plant-ui `split_batch_page` 同一口径）；对不上回 null。 */
function splitBatchPage(json, members) {
  const roots = json?.roots;
  const items = json?.items;
  if (!Array.isArray(roots) || !Array.isArray(items) || roots.length !== members.length) return null;
  const totals = [];
  let sum = 0;
  for (let i = 0; i < roots.length; i++) {
    const total = roots[i]?.total;
    if (!Number.isInteger(total) || total < 0) return null;
    if (String(roots[i]?.generation_root ?? '').replace('/', '_') !== members[i]) return null;
    totals.push(total);
    sum += total;
  }
  if (json.truncated ? items.length >= sum : items.length !== sum) return null;
  let offset = 0;
  let complete = 0;
  for (const total of totals) {
    if (offset + total > items.length) break;
    offset += total;
    complete++;
  }
  return { complete, restTotals: totals.slice(complete), pageRows: items.length };
}

/**
 * 按成员取：`lanes` 路同时在途；`batchMax` = 1 是一个成员一发（members），> 1 是批量形（batch）。
 * 与 plant-ui `model_records_fetch.rs` 同一调度：头几发按 RAMP，一组不超过「剩下的平摊到每一路」，
 * 一页装不下的按条数重新分组，批量请求没成或响应对不上就这一组逐个重问、余下不再打包。
 */
async function readMembers(acc, members, lanesWanted, batchMax) {
  const total = members.length;
  const lanes = Math.max(1, Math.min(lanesWanted, total));
  let batching = batchMax > 1 && total > 1;
  let next = 0;
  let issued = 0;
  const regrouped = [];
  const takeGroup = () => {
    const waiting = regrouped.shift();
    if (waiting) return waiting;
    if (next >= total) return null;
    const fairShare = Math.ceil((total - next) / lanes);
    const size = batching ? Math.max(1, Math.min(batchMax, RAMP[issued] ?? batchMax, fairShare)) : 1;
    issued++;
    const group = [];
    while (group.length < size && next < total) group.push(next++);
    return group;
  };
  const readGroup = async (group) => {
    if (group.length === 1 || !batching) {
      for (const index of group) await readSingle(acc, members[index]);
      return;
    }
    const names = group.map((index) => members[index]);
    const r = await postRecords({ generation_roots: names.map(v1), limit: PAGE });
    note(acc, r);
    const split = r.ok ? splitBatchPage(r.json, names) : null;
    if (!split) {
      // 没成 / 对不上：逐个重问，余下不再打包（这一发的错不算错——由单根那一发说了算）
      if (!r.ok) { acc.errors--; acc.errorText = null; }
      batching = false;
      for (const index of group) await readSingle(acc, members[index]);
      return;
    }
    let collected = 0;
    for (let i = 0; i < split.complete; i++) collected += r.json.roots[i].total;
    acc.items += collected;
    if (split.restTotals.length > 0) acc.regrouped++;
    let bucket = [];
    let bucketRows = 0;
    const flush = () => { if (bucket.length) regrouped.push(bucket); bucket = []; bucketRows = 0; };
    for (let i = 0; i < split.restTotals.length; i++) {
      const index = group[split.complete + i];
      const rows = split.restTotals[i];
      if (rows > split.pageRows) { flush(); regrouped.push([index]); continue; }
      if (bucketRows + rows > split.pageRows) flush();
      bucket.push(index);
      bucketRows += rows;
    }
    flush();
  };
  await Promise.all(Array.from({ length: lanes }, async () => {
    for (;;) {
      const group = takeGroup();
      if (!group) return;
      await readGroup(group);
    }
  }));
}

async function main() {
  const health = await getJson('/api/v1/health');
  const declared = health?.capabilities?.model_records_batch_max_roots;
  const batchCap = Number.isInteger(declared) && declared >= 2 ? Math.min(declared, MAX_ROOTS) : 0;
  console.log(`后端 ${health.version ?? '?'}（${health.build_id ?? '?'}），项目 ${health.project ?? '?'}，批量形上限 ${batchCap || '未声明'}`);

  const children = await getJson(`/api/v1/tree/children?refno=${encodeURIComponent(v1(KEY))}`);
  const nodes = children.nodes ?? [];
  const isSite = nodes.length > 0 && nodes.every((n) => String(n.noun).toUpperCase() === 'ZONE');
  const members = isSite ? nodes.map((n) => String(n.refno).replace('/', '_')) : [];
  const batchMax = isSite && members.length > MIN_MEMBERS_TO_BATCH && batchCap ? Math.min(WANT_BATCH, batchCap) : 1;
  console.log(`${KEY}：${isSite ? `SITE，${members.length} 个 ZONE` : `非 SITE（成员 ${nodes.length} 个，取法 members / batch 不适用）`}；页 ${PAGE} 条，${CONCURRENCY} 路；批量形一发 ${batchMax > 1 ? `${batchMax} 个` : '不打包'}`);

  const plan = STRATEGIES.filter((s) => {
    if (s === 'whole') return true;
    if (s === 'members') return isSite;
    if (s === 'batch') return isSite && batchMax > 1;
    console.error(`不认识的取法 ${s}，跳过`);
    return false;
  });
  const rows = [];
  for (let pass = 1; pass <= REPEAT; pass++) {
    for (const strategy of plan) {
      const acc = newAcc();
      if (strategy === 'whole') await readSingle(acc, KEY);
      else if (strategy === 'members') await readMembers(acc, members, CONCURRENCY, 1);
      else await readMembers(acc, members, CONCURRENCY, batchMax);
      const wallMs = performance.now() - acc.t0;
      const row = {
        at: new Date().toISOString(), refno: KEY, strategy, pass, members: members.length, batchMax: strategy === 'batch' ? batchMax : 1,
        wallMs: round(wallMs), firstMs: round(acc.firstMs ?? 0), requests: acc.requests, pages: acc.pages, bytes: acc.bytes, items: acc.items,
        regrouped: acc.regrouped, errors: acc.errors, errorText: acc.errorText,
        sumWaitMs: round(acc.sumWaitMs), sumDlMs: round(acc.sumDlMs), serverReadMs: round(acc.serverReadMs), serverTotalMs: round(acc.serverTotalMs),
      };
      rows.push(row);
      if (JSONL) appendFileSync(JSONL, `${JSON.stringify(row)}\n`);
      console.log(`#${pass} ${strategy.padEnd(7)} 墙钟 ${String(row.wallMs).padStart(8)} ms | 首发 ${String(row.firstMs).padStart(7)} ms | ${String(row.requests).padStart(4)} 发 ${(row.bytes / 1048576).toFixed(1).padStart(6)} MB ${String(row.items).padStart(6)} 条 | 服务端读/总 ${row.serverReadMs}/${row.serverTotalMs} ms${row.regrouped ? ` | 重分组 ${row.regrouped}` : ''}${row.errors ? ` | 错 ${row.errors}（${row.errorText}）` : ''}`);
      await new Promise((resolve) => setTimeout(resolve, 800));
    }
  }

  console.log('\n中位数（各取法 %d 次）：', REPEAT);
  console.log('| 取法 | 收齐 ms | 首发 ms | 请求数 | 字节 MB | 条数 | 服务端读 ms | 错 |');
  console.log('|---|---:|---:|---:|---:|---:|---:|---:|');
  for (const strategy of plan) {
    const mine = rows.filter((row) => row.strategy === strategy);
    const med = (key) => median(mine.map((row) => row[key]));
    console.log(`| ${strategy} | ${round(med('wallMs'))} | ${round(med('firstMs'))} | ${med('requests')} | ${(med('bytes') / 1048576).toFixed(1)} | ${med('items')} | ${round(med('serverReadMs'))} | ${mine.reduce((n, row) => n + row.errors, 0)} |`);
  }
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
});
