// Claude Storage Cleaner — front end. Vanilla JS, no build step.

const tauri = window.__TAURI__;
const invoke = tauri ? tauri.core.invoke : async () => { throw new Error("not running inside Tauri"); };
const listen = tauri ? tauri.event.listen : async () => () => {};

const state = {
  report: null,
  view: "overview", // "overview" | "claude" | repo path
  selected: null,   // worktree path
  scanning: false,
  progress: "",
  filter: "all",    // all | safe | caution | pooled
  sheet: null,      // { request, plan, running, result, ack, error }
};

// ---------- helpers ----------
const $ = (s) => document.querySelector(s);

function el(tag, attrs = {}, ...children) {
  const n = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") n.className = v;
    else if (k === "text") n.textContent = v;
    else if (k === "html") n.innerHTML = v;
    else if (k.startsWith("on")) n.addEventListener(k.slice(2), v);
    else if (v !== null && v !== undefined) n.setAttribute(k, v);
  }
  for (const c of children.flat()) {
    if (c === null || c === undefined) continue;
    n.append(c.nodeType ? c : document.createTextNode(String(c)));
  }
  return n;
}

function fmtBytes(n) {
  if (n == null) return "—";
  const u = ["B", "KB", "MB", "GB", "TB"];
  let v = n, i = 0;
  while (v >= 1000 && i < u.length - 1) { v /= 1000; i++; }
  if (i === 0) return `${n} B`;
  return `${v >= 100 ? v.toFixed(0) : v.toFixed(1)} ${u[i]}`;
}

function fmtAgo(ms) {
  if (!ms) return "—";
  const d = Date.now() - ms;
  const m = Math.round(d / 60000);
  if (m < 1) return "just now";
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} h ago`;
  const days = Math.round(h / 24);
  if (days < 30) return `${days} d ago`;
  return new Date(ms).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

function fmtDate(ms) {
  return ms ? new Date(ms).toLocaleString() : "—";
}

function stateKind(w) {
  return w.state.kind; // safe | pooled | caution | inUse
}

function stateLabel(w) {
  return { safe: "Safe", pooled: "Spare", caution: "Caution", inUse: "In use" }[w.state.kind];
}

function reasonText(r) {
  switch (r.type) {
    case "liveProcess": return `Open in a running session (pid ${r.pid})`;
    case "gitLocked": return `Locked by git${r.reason ? `: ${r.reason}` : ""}`;
    case "mainCheckout": return "Main checkout";
    case "dirty": return `${r.files} changed file${r.files === 1 ? "" : "s"}`;
    case "untracked": return `${r.files} untracked file${r.files === 1 ? "" : "s"}`;
    case "unpushed": return `${r.commits} unpushed commit${r.commits === 1 ? "" : "s"}`;
    case "noUpstreamWithCommits": return `${r.commits} commit${r.commits === 1 ? "" : "s"} on a branch with no upstream`;
    case "detachedWithOwnCommits": return `${r.commits} commit${r.commits === 1 ? "" : "s"} not on any branch`;
    case "openInApp": return "Session still open in the Claude app";
    case "unregistered": return "Not created by the Claude app";
    case "orphan": return "Unknown to git";
    case "modifiedRecently": return `Modified ${r.minutes} min ago`;
    case "gitError": return `git: ${r.message}`;
    default: return r.type;
  }
}

function reasons(w) {
  return (w.state.reasons || []).map(reasonText);
}

function artifactSummary(w) {
  const m = new Map();
  for (const a of w.artifacts) m.set(a.kind, (m.get(a.kind) || 0) + a.bytes);
  return [...m.entries()].sort((a, b) => b[1] - a[1]);
}

function chips(w) {
  const all = artifactSummary(w);
  const shown = all.slice(0, 2).map(([k, b]) => el("span", { class: "chip", text: `${KIND_LABEL[k] || k} ${fmtBytes(b)}` }));
  if (all.length > 2) {
    const rest = all.slice(2);
    shown.push(el("span", { class: "chip", title: rest.map(([k, b]) => `${KIND_LABEL[k] || k} ${fmtBytes(b)}`).join("\n"), text: `+${rest.length}` }));
  }
  return el("div", { class: "chips" }, ...shown);
}

const KIND_LABEL = { terraform: ".terraform", nodeModules: "node_modules", cargoTarget: "target", next: ".next", turbo: ".turbo", dist: "dist" };

function allWorktrees() {
  const out = [];
  for (const r of state.report?.repos || []) {
    for (const w of r.worktrees) out.push({ w, repo: r });
  }
  return out.sort((a, b) => b.w.bytesTotal - a.w.bytesTotal);
}

function findWorktree(path) {
  for (const r of state.report?.repos || []) {
    if (r.main.path === path) return { w: r.main, repo: r };
    const w = r.worktrees.find((x) => x.path === path);
    if (w) return { w, repo: r };
  }
  return null;
}

// ---------- rendering ----------
function render() {
  renderSidebar();
  renderView();
  renderDetail();
  $("#progress").hidden = !state.scanning;
  $("#toolbar-note").textContent = state.scanning ? state.progress : "";
  if (state.sheet) renderSheet();
}

function renderSidebar() {
  for (const a of document.querySelectorAll(".nav-item[data-view]")) {
    a.classList.toggle("is-active", a.dataset.view === state.view);
  }
  const list = $("#repo-list");
  list.replaceChildren();
  for (const r of state.report?.repos || []) {
    list.append(
      el("a", {
        class: `nav-item${state.view === r.path ? " is-active" : ""}`,
        onclick: () => { state.view = r.path; state.selected = null; render(); },
      },
        el("span", { class: "label", text: r.name }),
        el("span", { class: "size", text: fmtBytes(r.totalBytes) }),
      ),
    );
  }
  const at = state.report?.scannedAt;
  $("#scanned-at").textContent = at ? `Scanned ${fmtAgo(at)} in ${(state.report.durationMs / 1000).toFixed(0)} s` : "Not scanned yet";
  $("#rescan").textContent = state.scanning ? "Cancel" : "Rescan";
}

function renderView() {
  const view = $("#view");
  view.replaceChildren();
  if (!state.report) {
    $("#view-title").textContent = "Overview";
    view.append(el("div", { class: "empty", text: state.scanning ? "Scanning…" : "No scan yet." }));
    return;
  }
  if (state.view === "overview") return renderOverview(view);
  if (state.view === "claude") return renderClaude(view);
  const repo = state.report.repos.find((r) => r.path === state.view);
  if (!repo) { state.view = "overview"; return renderOverview(view); }
  renderRepo(view, repo);
}

function tiles(t) {
  return el("div", { class: "tiles" },
    tile(fmtBytes(t.claudeBytes), "Claude on disk"),
    tile(fmtBytes(t.reclaimableBytes), "Reclaimable now"),
    tile(fmtBytes(t.artifactBytes), "Build artifacts"),
    tile(String(t.worktreeCount), "Worktrees"),
  );
}

function tile(v, l) {
  return el("div", { class: "tile" }, el("div", { class: "tile-value", text: v }), el("div", { class: "tile-label", text: l }));
}

function filterBar() {
  const opts = [["all", "All"], ["safe", "Safe"], ["pooled", "Spare"], ["caution", "Caution"]];
  return el("div", { class: "filters" },
    ...opts.map(([k, l]) => el("button", {
      class: `seg${state.filter === k ? " is-on" : ""}`,
      text: l,
      onclick: () => { state.filter = k; render(); },
    })),
  );
}

function applyFilter(rows) {
  if (state.filter === "all") return rows;
  return rows.filter(({ w }) => w.isMain || stateKind(w) === state.filter);
}

function renderOverview(view) {
  $("#view-title").textContent = "Overview";
  view.append(tiles(state.report.totals));
  const rows = applyFilter(allWorktrees());
  view.append(el("div", { class: "row-head" }, el("div", { class: "section-title", text: "Worktrees" }), filterBar()));
  view.append(table(rows, { showRepo: true }));
}

function renderRepo(view, repo) {
  $("#view-title").textContent = repo.name;
  const rows = applyFilter([{ w: repo.main, repo }, ...repo.worktrees.map((w) => ({ w, repo }))]);
  view.append(el("div", { class: "row-head" },
    el("div", { class: "muted", text: `${repo.worktrees.length} worktrees · ${fmtBytes(repo.totalBytes)}${repo.staleEntries.length ? ` · ${repo.staleEntries.length} stale git entries` : ""}` }),
    filterBar(),
  ));
  view.append(table(rows, { showRepo: false }));
}

function table(rows, { showRepo }) {
  if (!rows.length) return el("div", { class: "empty", text: "Nothing here." });
  const max = Math.max(1, ...rows.map(({ w }) => w.bytesTotal));
  const head = el("tr", {},
    el("th", { style: "width:34%", text: "Name" }),
    el("th", { class: "num", style: "width:150px", text: "Size" }),
    el("th", { text: "Artifacts" }),
    el("th", { style: "width:90px", text: "State" }),
    el("th", { class: "col-last", style: "width:100px", text: "Last activity" }),
  );
  const body = rows.map(({ w, repo }) => {
    const sub = w.isMain ? "main checkout" : [showRepo ? repo.name : null, w.branch || (w.detached ? "detached" : "")].filter(Boolean).join(" · ");
    const last = w.sessions[0]?.lastActivity || w.lastModified;
    return el("tr", {
      class: `row${state.selected === w.path ? " is-selected" : ""}${w.isMain ? " is-main" : ""}`,
      onclick: () => { state.selected = state.selected === w.path ? null : w.path; render(); },
    },
      el("td", {}, el("span", { class: "name", text: w.name }), el("span", { class: "sub", text: sub })),
      el("td", { class: "num" }, el("div", { class: "size-cell" },
        el("div", { class: "bar" }, el("i", { style: `width:${Math.max(2, (100 * w.bytesTotal) / max)}%` })),
        fmtBytes(w.bytesTotal),
      )),
      el("td", {}, chips(w)),
      el("td", {}, el("span", { class: `badge ${stateKind(w)}`, title: reasons(w).join("\n"), text: stateLabel(w) })),
      el("td", { class: "muted col-last", text: fmtAgo(last) }),
    );
  });
  return el("table", { class: "table" }, el("thead", {}, head), el("tbody", {}, ...body));
}

function renderClaude(view) {
  $("#view-title").textContent = "Claude data";
  const dirs = state.report.transcriptDirs;
  const forgettable = dirs.filter((d) => d.forgettable);
  view.append(el("div", { class: "row-head" },
    el("div", { class: "section-title", text: "Transcripts" }),
    el("div", { style: "display:flex;align-items:center;gap:12px" },
      el("span", { class: "muted", text: `${dirs.length} folders` }),
      forgettable.length ? el("button", {
        class: "btn small",
        text: `Forget ${forgettable.length} for missing folders… (${fmtBytes(forgettable.reduce((a, d) => a + d.bytes, 0))})`,
        onclick: () => openSheet({ type: "forgetTranscripts", dirs: forgettable.map((d) => d.path), permanent: false }),
      }) : null),
  ));
  const max = Math.max(1, ...dirs.map((d) => d.bytes));
  view.append(el("table", { class: "table" },
    el("thead", {}, el("tr", {},
      el("th", { style: "width:50%", text: "Working directory" }),
      el("th", { class: "num", style: "width:150px", text: "Size" }),
      el("th", { class: "num", style: "width:80px", text: "Sessions" }),
      el("th", { text: "Status" }),
    )),
    el("tbody", {}, ...dirs.map((d) => el("tr", { class: "row" },
      el("td", {}, el("span", { class: "name", text: d.cwd ? d.cwd.replace(/^\/Users\/[^/]+/, "~") : d.name }),
        el("span", { class: "sub", text: d.sessions[0]?.title || "" })),
      el("td", { class: "num" }, el("div", { class: "size-cell" },
        el("div", { class: "bar" }, el("i", { style: `width:${Math.max(2, (100 * d.bytes) / max)}%` })), fmtBytes(d.bytes))),
      el("td", { class: "num", text: String(d.sessions.length) }),
      el("td", {}, d.cwd == null
        ? el("span", { class: "badge inUse", text: "Unknown" })
        : d.cwdExists
          ? el("span", { class: "badge pooled", text: "Folder exists" })
          : el("span", { class: `badge ${d.forgettable ? "safe" : "caution"}`, text: d.forgettable ? "Folder gone" : "Folder gone, in use" })),
    ))),
  ));

  view.append(el("div", { class: "section-title", text: "Other storage" }));
  const bmax = Math.max(1, ...state.report.buckets.map((b) => b.bytes));
  view.append(el("table", { class: "table" },
    el("thead", {}, el("tr", {},
      el("th", { style: "width:34%", text: "Location" }),
      el("th", { class: "num", style: "width:150px", text: "Size" }),
      el("th", { text: "Note" }),
    )),
    el("tbody", {}, ...state.report.buckets.map((b) => el("tr", { class: "row", onclick: () => invoke("reveal", { path: b.path }).catch(() => {}) },
      el("td", {}, el("span", { class: "name", text: b.label }), el("span", { class: "sub", text: b.path.replace(/^\/Users\/[^/]+/, "~") })),
      el("td", { class: "num" }, el("div", { class: "size-cell" },
        el("div", { class: "bar" }, el("i", { style: `width:${Math.max(2, (100 * b.bytes) / bmax)}%` })),
        b.apparentBytes ? `${fmtBytes(b.bytes)} of ${fmtBytes(b.apparentBytes)}` : fmtBytes(b.bytes))),
      el("td", { class: "muted", text: b.note || (b.claude ? "Claude" : "") }),
    ))),
  ));
}

function kv(pairs) {
  const dl = el("dl", { class: "kv" });
  for (const [k, v] of pairs) {
    if (v === null || v === undefined || v === "") continue;
    dl.append(el("dt", { text: k }), el("dd", { text: String(v), title: String(v) }));
  }
  return dl;
}

function renderDetail() {
  const pane = $("#detail");
  const hit = state.selected && findWorktree(state.selected);
  document.querySelector(".body").classList.toggle("has-detail", !!hit && state.view !== "claude");
  if (!hit || state.view === "claude") { pane.hidden = true; pane.replaceChildren(); return; }
  const { w, repo } = hit;
  pane.hidden = false;
  pane.replaceChildren();
  pane.append(el("h2", { text: w.isMain ? `${repo.name} (main)` : w.name }));
  pane.append(el("div", { class: "path", text: w.path }));
  pane.append(el("div", { style: "margin-top:8px;display:flex;gap:8px" },
    el("button", { class: "btn small", text: "Reveal in Finder", onclick: () => invoke("reveal", { path: w.path }).catch(() => {}) }),
  ));
  const rs = reasons(w);
  pane.append(el("div", { class: "reasons" },
    el("div", {}, el("span", { class: `badge ${stateKind(w)}`, text: stateLabel(w) })),
    rs.length ? el("ul", { style: "margin:6px 0 0;padding-left:16px" }, ...rs.map((r) => el("li", { text: r }))) :
      el("div", { class: "muted", style: "margin-top:4px", text: stateKind(w) === "safe" ? "Clean, every commit is on a remote, no open session." : stateKind(w) === "pooled" ? "The Claude app keeps this as a spare for new sessions." : "" }),
  ));
  pane.append(kv([
    ["Size", `${fmtBytes(w.bytesTotal)} (${fmtBytes(w.bytesCheckout)} checkout)`],
    ["Branch", w.branch || (w.detached ? "detached HEAD" : "—")],
    ["From", w.sourceBranch],
    ["HEAD", w.head ? w.head.slice(0, 10) : null],
    ["Changed", w.git.dirty + w.git.untracked ? `${w.git.dirty} changed, ${w.git.untracked} untracked` : "clean"],
    ["Unpushed", w.git.unpushed.kind === "count" ? String(w.git.unpushed.count) : `no upstream, ${w.git.ownCommits} own commit${w.git.ownCommits === 1 ? "" : "s"}`],
    ["Not on remote", w.git.notOnRemote],
    ["Created", w.createdAt ? fmtDate(w.createdAt) : null],
    ["Modified", fmtDate(w.lastModified)],
    ["Registry", w.registry ? (w.registry.leasedBy ? "leased" : "spare pool") : "not registered"],
  ]));
  if (w.artifacts.length) {
    pane.append(el("div", { class: "section-title", text: "Artifacts" }));
    const grouped = artifactSummary(w);
    pane.append(el("ul", { class: "list" }, ...grouped.map(([k, b]) => el("li", {},
      el("span", { text: `${KIND_LABEL[k] || k} × ${w.artifacts.filter((a) => a.kind === k).length}` }),
      el("span", { class: "num", text: fmtBytes(b) })))));
  }
  if (w.sessions.length) {
    pane.append(el("div", { class: "section-title", text: `Sessions (${w.sessions.length})` }));
    pane.append(el("ul", { class: "list" }, ...w.sessions.slice(0, 8).map((s) => el("li", {},
      el("span", { text: `${s.title || "Untitled"}${s.live ? " · running" : s.archived ? "" : " · open"}`, title: s.cliId || "" }),
      el("span", { class: "num", text: fmtAgo(s.lastActivity) })))));
  }
  const prunable = w.artifacts.filter((a) => !a.tracked);
  const inUse = stateKind(w) === "inUse";
  pane.append(el("div", { class: "actions" },
    el("button", {
      class: "btn",
      disabled: !w.pruneAllowed || !prunable.length ? "" : null,
      title: !w.pruneAllowed ? "In use" : !prunable.length ? "No build artifacts" : "",
      text: prunable.length ? `Prune artifacts… (${fmtBytes(prunable.reduce((a, x) => a + x.bytes, 0))})` : "Prune artifacts…",
      onclick: () => openSheet({ type: "pruneArtifacts", worktree: w.path, kinds: [...new Set(prunable.map((a) => a.kind))], permanent: false }),
    }),
    w.isMain ? null : el("button", {
      class: "btn danger",
      disabled: inUse ? "" : null,
      title: inUse ? reasons(w).join("\n") : "",
      text: "Remove worktree…",
      onclick: () => openSheet({ type: "removeWorktree", worktree: w.path, deleteBranch: false, permanent: false }),
    }),
  ));
}

// ---------- action sheet ----------
async function openSheet(request) {
  state.sheet = { request, plan: null, running: false, result: null, ack: false, error: null };
  renderSheet();
  await previewSheet();
}

async function previewSheet() {
  const sh = state.sheet;
  if (!sh) return;
  try {
    sh.plan = await invoke("preview_action", { request: sh.request });
    sh.error = null;
  } catch (e) {
    sh.error = String(e);
  }
  renderSheet();
}

async function rescanSheet() {
  const sh = state.sheet;
  if (!sh) return;
  const repo = sh.plan?.repo || findWorktree(sh.request.worktree)?.repo.path;
  sh.error = null;
  sh.plan = null;
  renderSheet();
  try {
    state.report = repo ? await invoke("refresh_repo", { path: repo }) : await invoke("get_report");
  } catch (e) {
    sh.error = String(e);
  }
  render();
  await previewSheet();
}

function closeSheet() {
  state.sheet = null;
  renderSheet();
}

async function runSheet() {
  const sh = state.sheet;
  if (!sh?.plan || sh.running) return;
  sh.running = true;
  renderSheet();
  try {
    sh.result = await invoke("run_action", { plan: sh.plan, acknowledged: sh.ack });
  } catch (e) {
    sh.error = String(e);
  }
  sh.running = false;
  renderSheet();
}

async function finishSheet() {
  const sh = state.sheet;
  const plan = sh?.plan;
  closeSheet();
  try {
    if (plan?.request.type === "forgetTranscripts") {
      state.report = await invoke("get_report");
    } else if (plan?.repo) {
      state.report = await invoke("refresh_repo", { path: plan.repo });
    }
  } catch (e) {
    console.error(e);
  }
  if (state.selected && !findWorktree(state.selected)) state.selected = null;
  render();
}

function sheetTitle(req) {
  const name = (p) => p.split("/").pop();
  switch (req.type) {
    case "pruneArtifacts": return `Prune artifacts in ${name(req.worktree)}`;
    case "removeWorktree": return `Remove ${name(req.worktree)}`;
    case "forgetTranscripts": return `Forget ${req.dirs.length} transcript folder${req.dirs.length === 1 ? "" : "s"}`;
  }
}

function renderSheet() {
  document.querySelector(".overlay")?.remove();
  const sh = state.sheet;
  if (!sh) return;
  const plan = sh.plan;
  const body = el("div", { class: "sheet-body" });
  if (sh.error) body.append(el("div", { class: "note blocker", text: sh.error }));
  if (plan) {
    body.append(el("ul", { class: "steps" }, ...plan.steps.map((s) => el("li", {},
      el("div", { class: "what" }, s.label, el("span", { class: "path", text: s.kind === "trash" ? `${s.path} → Trash` : s.kind === "delete" ? `${s.path} (delete)` : s.path })),
      el("span", { class: "num", text: s.bytes ? fmtBytes(s.bytes) : "" })))));
    for (const b of plan.blockers) body.append(el("div", { class: "note blocker", text: b }));
    for (const w of plan.warnings) body.append(el("div", { class: "note", text: w }));
    if (!sh.result) {
      const req = sh.request;
      if (req.type === "removeWorktree") {
        body.append(el("label", { class: "option" },
          el("input", { type: "checkbox", ...(req.deleteBranch ? { checked: "" } : {}), onchange: (e) => { req.deleteBranch = e.target.checked; previewSheet(); } }),
          "Also delete the branch when every commit is on a remote"));
      }
      body.append(el("label", { class: "option" },
        el("input", { type: "checkbox", ...(req.permanent ? { checked: "" } : {}), onchange: (e) => { req.permanent = e.target.checked; previewSheet(); } }),
        "Delete immediately instead of moving to the Trash"));
      if (plan.discardsWork) {
        body.append(el("label", { class: "option" },
          el("input", { type: "checkbox", ...(sh.ack ? { checked: "" } : {}), onchange: (e) => { sh.ack = e.target.checked; renderSheet(); } }),
          el("b", { text: "I understand this discards work that exists nowhere else" })));
      }
    }
  }
  if (sh.result) {
    const r = sh.result;
    if (r.failed) {
      body.append(el("div", { class: "result failed" }, `Stopped at "${r.failed.step.label}": ${r.failed.message}`));
    } else {
      const parts = [];
      if (r.bytesTrashed) parts.push(`${fmtBytes(r.bytesTrashed)} moved to the Trash. Empty the Trash to reclaim the space.`);
      if (r.bytesFreed) parts.push(`${fmtBytes(r.bytesFreed)} deleted.`);
      if (!parts.length) parts.push("Done.");
      body.append(el("div", { class: "result" }, parts.join(" ")));
    }
  }
  const canRun = plan && !plan.blockers.length && plan.steps.length && (!plan.discardsWork || sh.ack) && !sh.running && !sh.result && !state.scanning;
  const stale = /rescan/i.test(sh.error || "") || (plan && plan.blockers.some((b) => /rescan/i.test(b)));
  const foot = el("div", { class: "sheet-foot" },
    el("span", { class: "grow", text: state.scanning ? "Scanning… actions run when it finishes" : plan && !sh.result ? (sh.running ? "Working…" : `${plan.steps.length} step${plan.steps.length === 1 ? "" : "s"} · ${fmtBytes(plan.bytes)}`) : "" }),
    stale && !sh.result ? el("button", { class: "btn", text: "Rescan", onclick: rescanSheet }) : null,
    sh.result && sh.result.bytesTrashed ? el("button", { class: "btn", text: "Open Trash", onclick: () => invoke("open_trash").catch(() => {}) }) : null,
    sh.result
      ? el("button", { class: "btn primary", text: "Done", onclick: finishSheet })
      : [el("button", { class: "btn", text: "Cancel", disabled: sh.running ? "" : null, onclick: closeSheet }),
         el("button", { class: `btn ${plan?.discardsWork ? "danger" : "primary"}`, text: sh.request.type === "removeWorktree" ? "Remove" : sh.request.type === "forgetTranscripts" ? "Forget" : "Prune", disabled: canRun ? null : "", onclick: runSheet })],
  );
  const sheet = el("div", { class: "sheet" },
    el("div", { class: "sheet-head" }, el("h3", { text: sheetTitle(sh.request) }), el("div", { class: "muted", text: plan ? "" : sh.error ? "" : "Preparing…" })),
    body, foot);
  const overlay = el("div", { class: "overlay", onclick: (e) => { if (e.target === overlay && !sh.running && !sh.result) closeSheet(); } }, sheet);
  document.body.append(overlay);
}

// ---------- events ----------
async function startScan() {
  if (state.scanning) { await invoke("cancel_scan"); return; }
  state.scanning = true;
  state.progress = "Starting…";
  render();
  try { await invoke("start_scan", { roots: null }); }
  catch (e) { state.scanning = false; state.progress = ""; render(); alert(String(e)); }
}

function mergeRepo(repo) {
  if (!state.report) state.report = { repos: [], transcriptDirs: [], buckets: [], totals: { claudeBytes: 0, reclaimableBytes: 0, artifactBytes: 0, worktreeCount: 0 }, scannedAt: null, durationMs: 0 };
  const i = state.report.repos.findIndex((r) => r.path === repo.path);
  if (i >= 0) state.report.repos[i] = repo; else state.report.repos.push(repo);
  state.report.repos.sort((a, b) => b.totalBytes - a.totalBytes);
}

async function init() {
  $("#rescan").addEventListener("click", startScan);
  for (const a of document.querySelectorAll(".nav-item[data-view]")) {
    a.addEventListener("click", () => { state.view = a.dataset.view; state.selected = null; render(); });
  }
  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    if (state.sheet) { if (!state.sheet.running) closeSheet(); return; }
    state.selected = null; render();
  });

  await listen("scan-progress", (ev) => {
    const { phase, current } = ev.payload;
    const names = { sessions: "Reading sessions", transcripts: "Reading transcripts", worktrees: "Measuring", buckets: "Measuring caches" };
    state.progress = `${names[phase] || phase}${current ? ` · ${current.split("/").pop()}` : ""}`;
    $("#toolbar-note").textContent = state.progress;
  });
  await listen("scan-repo", (ev) => { mergeRepo(ev.payload); render(); });
  await listen("scan-done", (ev) => { state.report = ev.payload; state.scanning = false; state.progress = ""; render(); });
  await listen("scan-cancelled", () => { state.scanning = false; state.progress = ""; render(); });

  try {
    state.report = await invoke("get_report");
  } catch (e) {
    state.report = null;
  }
  render();
  startScan();
}

init();
