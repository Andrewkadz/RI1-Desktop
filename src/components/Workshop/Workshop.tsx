// RI1 Desktop — Workshop tab
// Tune, test, inspect and collect training material for the local model.
// Nothing here changes model weights: Tuning and the Test bench change how a
// model is driven; the Notebook collects question→answer pairs for retraining;
// Surgery shows what Ollama reports about a model and flags known problems.
import { useEffect, useMemo, useState } from "react";
import type { CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";

// ------------------------------------------------------------------ types
type Options = {
  temperature: number;
  top_p: number;
  min_p: number;
  repeat_penalty: number;
  num_predict: number;
  num_ctx: number;
  seed: number | null;
};

type RunStats = {
  model: string;
  totalMs: number;
  loadMs: number;
  promptTokens: number;
  replyTokens: number;
  tokensPerSec: number;
  doneReason: string;
};

type ChatResult = { content: string; stats: RunStats };

type NoteEntry = {
  id: string;
  created: string;
  question: string;
  answer: string;
  source: "kept" | "corrected" | "manual";
  model?: string;
  options?: Partial<Options>;
};

type BenchCell = { content: string; stats?: RunStats; error?: string };

// ------------------------------------------------------------------ constants
const DEFAULT_OPTIONS: Options = {
  temperature: 0.4,
  top_p: 0.9,
  min_p: 0.05,
  repeat_penalty: 1.1,
  num_predict: 768,
  num_ctx: 8192,
  seed: null,
};

const SLIDERS: {
  key: keyof Omit<Options, "seed">;
  label: string;
  min: number;
  max: number;
  step: number;
  help: string;
}[] = [
  { key: "temperature", label: "Temperature", min: 0, max: 1.5, step: 0.05,
    help: "Randomness. Low = predictable, repeats the safest wording. High = varied, more likely to wander or invent." },
  { key: "top_p", label: "Top-p", min: 0.1, max: 1, step: 0.05,
    help: "Only consider the most likely words that together make up this share of probability. Lower = narrower vocabulary." },
  { key: "min_p", label: "Min-p", min: 0, max: 0.3, step: 0.01,
    help: "Drop any word less than this fraction as likely as the best word. Cuts off long-shot nonsense." },
  { key: "repeat_penalty", label: "Repeat penalty", min: 1, max: 1.5, step: 0.01,
    help: "Discourages reusing recent words. Too low = loops (like 'Архів//Архів'); too high = awkward word choices." },
  { key: "num_predict", label: "Max reply length (tokens)", min: 32, max: 2048, step: 32,
    help: "Hard cap on reply length. A token is roughly ¾ of a word. The safety net if the model never stops." },
  { key: "num_ctx", label: "Context window (tokens)", min: 2048, max: 8192, step: 512,
    help: "How much text (instructions + documents + question) the model can see at once. Bigger uses more memory." },
];

const MODEL_INFO_HELP: Record<string, string> = {
  "general.architecture": "The network design family. 'llama' = Meta's Llama transformer layout.",
  "general.parameter_count": "Number of learned numbers (weights). More generally means more capable and more memory.",
  context_length: "The longest input the model was built to handle, in tokens.",
  embedding_length: "Width of the model's internal representation of each token.",
  block_count: "Number of stacked transformer layers. Each layer refines the representation.",
  "attention.head_count": "Parallel 'attention heads' per layer — each learns to look at different relationships between words.",
  "attention.head_count_kv": "Heads that store memory for past words. Fewer than head_count = grouped attention, which saves memory.",
  vocab_size: "How many distinct tokens (word pieces) the model knows.",
  "rope.freq_base": "Setting for how position in the text is encoded. Affects how well long contexts work.",
};

const DETAIL_HELP: Record<string, string> = {
  family: "Model family the weights belong to.",
  parameter_size: "Rounded size of the model.",
  quantization_level: "How the weights are compressed. Q4 ≈ 4.5 bits per weight (small, slightly less accurate); Q8 ≈ 8.5 bits (bigger, closer to original); F16 = uncompressed.",
  format: "File format Ollama runs (GGUF).",
  parent_model: "The model this one was built FROM in its Modelfile.",
};

// ------------------------------------------------------------------ helpers
async function api<T = any>(path: string, body?: unknown): Promise<T> {
  const raw = await invoke<string>("ollama_api", {
    path,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  let v: any;
  try {
    v = JSON.parse(raw);
  } catch {
    throw new Error(`Unexpected reply from Ollama: ${raw.slice(0, 200)}`);
  }
  if (v && typeof v.error === "string") throw new Error(v.error);
  return v as T;
}

function cleanOptions(o: Options) {
  const out: Record<string, number> = {
    temperature: o.temperature,
    top_p: o.top_p,
    min_p: o.min_p,
    repeat_penalty: o.repeat_penalty,
    num_predict: o.num_predict,
    num_ctx: o.num_ctx,
  };
  if (o.seed !== null && Number.isFinite(o.seed)) out.seed = o.seed;
  return out;
}

async function chatOnce(args: {
  model: string;
  prompt: string;
  system?: string;
  options?: Options;
  keepAlive?: string | number;
}): Promise<ChatResult> {
  const messages: { role: string; content: string }[] = [];
  if (args.system !== undefined && args.system.trim() !== "") {
    messages.push({ role: "system", content: args.system });
  }
  messages.push({ role: "user", content: args.prompt });
  const body: Record<string, unknown> = { model: args.model, messages, stream: false };
  if (args.options) body.options = cleanOptions(args.options);
  if (args.keepAlive !== undefined) body.keep_alive = args.keepAlive;
  const v = await api<any>("chat", body);
  const ns = (x: unknown) => (typeof x === "number" ? x / 1e6 : 0);
  const evalMs = ns(v.eval_duration);
  const replyTokens = typeof v.eval_count === "number" ? v.eval_count : 0;
  return {
    content: v?.message?.content ?? "",
    stats: {
      model: args.model,
      totalMs: ns(v.total_duration),
      loadMs: ns(v.load_duration),
      promptTokens: typeof v.prompt_eval_count === "number" ? v.prompt_eval_count : 0,
      replyTokens,
      tokensPerSec: evalMs > 0 ? replyTokens / (evalMs / 1000) : 0,
      doneReason: v.done_reason ?? "",
    },
  };
}

function parseParameters(text: string | undefined): Partial<Options> {
  const out: Partial<Options> = {};
  if (!text) return out;
  for (const line of text.split("\n")) {
    const m = line.trim().match(/^(\w+)\s+(.+)$/);
    if (!m) continue;
    const [, k, v] = m;
    const n = Number(v.replace(/"/g, ""));
    if (!Number.isFinite(n)) continue;
    if (k in DEFAULT_OPTIONS) (out as any)[k] = n;
  }
  return out;
}

function parseJsonl<T>(text: string): T[] {
  return text
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean)
    .flatMap((l) => {
      try {
        return [JSON.parse(l) as T];
      } catch {
        return [];
      }
    });
}

function fmtMs(ms: number) {
  return ms >= 1000 ? `${(ms / 1000).toFixed(1)} s` : `${Math.round(ms)} ms`;
}

function fmtBytes(n: number) {
  if (!n) return "0";
  const u = ["B", "KB", "MB", "GB"];
  let i = 0;
  while (n >= 1024 && i < u.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n.toFixed(i >= 2 ? 1 : 0)} ${u[i]}`;
}

function newId() {
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

// ------------------------------------------------------------------ styles
const panel: CSSProperties = { border: "1px solid #333", borderRadius: 8, padding: 14, background: "#0a0a0a" };
const subtle: CSSProperties = { fontSize: 12, opacity: 0.7 };
const pre: CSSProperties = {
  whiteSpace: "pre-wrap",
  wordBreak: "break-word",
  background: "#111",
  border: "1px solid #2a2a2a",
  borderRadius: 6,
  padding: 10,
  margin: 0,
  fontSize: 13,
  maxHeight: 360,
  overflow: "auto",
};

function StatsLine({ s }: { s?: RunStats }) {
  if (!s) return null;
  return (
    <div style={{ ...subtle, display: "flex", gap: 12, flexWrap: "wrap" }}>
      <span>{s.replyTokens} tokens</span>
      <span>{s.tokensPerSec.toFixed(1)} tok/s</span>
      <span>prompt {s.promptTokens} tokens</span>
      <span>total {fmtMs(s.totalMs)}</span>
      {s.loadMs > 500 && <span>model load {fmtMs(s.loadMs)}</span>}
      {s.doneReason && (
        <span title="'stop' = the model ended on its own. 'length' = it hit the max-length cap (it may never have stopped).">
          ended: {s.doneReason === "length" ? "⚠ hit length cap" : s.doneReason}
        </span>
      )}
    </div>
  );
}

// ------------------------------------------------------------------ component
export function Workshop({ defaultModel }: { defaultModel: string }) {
  const [section, setSection] = useState<"tuning" | "bench" | "notebook" | "surgery">("tuning");
  const [models, setModels] = useState<string[]>([]);
  const [modelsError, setModelsError] = useState<string>("");
  const [model, setModel] = useState(defaultModel);
  const [workshopDir, setWorkshopDir] = useState("");

  // tuning state
  const [options, setOptions] = useState<Options>(DEFAULT_OPTIONS);
  const [system, setSystem] = useState("");
  const [modelSystem, setModelSystem] = useState("");
  const [modelParams, setModelParams] = useState<Partial<Options>>({});
  const [tunePrompt, setTunePrompt] = useState("");
  const [tuneResult, setTuneResult] = useState<ChatResult | null>(null);
  const [tuneBusy, setTuneBusy] = useState(false);
  const [tuneError, setTuneError] = useState("");
  const [lastStats, setLastStats] = useState<RunStats | undefined>();

  // bench state
  const [benchQuestions, setBenchQuestions] = useState(
    "In exactly two sentences, what is a recursive function?\nWhat is the Φπε operator Λ used for?\nList three RI1 core documents and say what each covers."
  );
  const [benchModels, setBenchModels] = useState<string[]>([]);
  const [benchUseTuning, setBenchUseTuning] = useState(false);
  const [benchResults, setBenchResults] = useState<Record<string, Record<number, BenchCell>>>({});
  const [benchBusy, setBenchBusy] = useState(false);
  const [benchProgress, setBenchProgress] = useState("");

  // notebook state
  const [notes, setNotes] = useState<NoteEntry[]>([]);
  const [notesError, setNotesError] = useState("");
  const [editing, setEditing] = useState<NoteEntry | null>(null);

  // surgery state
  const [show, setShow] = useState<any>(null);
  const [ps, setPs] = useState<any>(null);
  const [surgeryError, setSurgeryError] = useState("");

  // ------------------------------------------------ loading
  async function refreshModels() {
    try {
      const v = await api<{ models?: { name: string }[] }>("tags");
      const names = (v.models ?? []).map((m) => m.name).sort();
      setModels(names);
      setModelsError("");
      if (!names.includes(model) && names.length) {
        const preferred = names.find((n) => n === defaultModel) ?? names[0];
        setModel(preferred);
      }
      setBenchModels((prev) => (prev.length ? prev : names.filter((n) => n === defaultModel || n === "ri1-8b:desktop").slice(0, 2)));
    } catch (e: any) {
      setModelsError(String(e?.message ?? e));
    }
  }

  async function loadModelSettings(name: string, resetTuning = true) {
    try {
      const v = await api<any>("show", { model: name, name });
      setShow(v);
      setSurgeryError("");
      if (!resetTuning) return;
      const sys = typeof v.system === "string" ? v.system.trim() : "";
      setModelSystem(sys);
      setSystem(sys);
      const p = parseParameters(v.parameters);
      setModelParams(p);
      setOptions({ ...DEFAULT_OPTIONS, ...p });
    } catch (e: any) {
      setSurgeryError(String(e?.message ?? e));
    }
  }

  async function refreshPs() {
    try {
      setPs(await api<any>("ps"));
    } catch (e: any) {
      setSurgeryError(String(e?.message ?? e));
    }
  }

  async function loadNotes() {
    try {
      const text = await invoke<string>("workshop_read", { file: "notebook.jsonl" });
      setNotes(parseJsonl<NoteEntry>(text));
      setNotesError("");
    } catch (e: any) {
      setNotesError(String(e?.message ?? e));
    }
  }

  async function saveAllNotes(next: NoteEntry[]) {
    setNotes(next);
    try {
      await invoke("workshop_write", {
        file: "notebook.jsonl",
        content: next.map((n) => JSON.stringify(n)).join("\n") + (next.length ? "\n" : ""),
      });
      setNotesError("");
    } catch (e: any) {
      setNotesError(String(e?.message ?? e));
    }
  }

  async function addNote(entry: Omit<NoteEntry, "id" | "created">) {
    const full: NoteEntry = { ...entry, id: newId(), created: new Date().toISOString() };
    try {
      await invoke("workshop_append", { file: "notebook.jsonl", line: JSON.stringify(full) });
      setNotes((prev) => [...prev, full]);
      setNotesError("");
    } catch (e: any) {
      setNotesError(String(e?.message ?? e));
    }
  }

  useEffect(() => {
    void refreshModels();
    void loadNotes();
    invoke<string>("workshop_dir").then(setWorkshopDir).catch(() => undefined);
  }, []);

  useEffect(() => {
    if (model) void loadModelSettings(model);
  }, [model]);

  useEffect(() => {
    if (section === "surgery") void refreshPs();
  }, [section]);

  // ------------------------------------------------ actions
  async function runTuning() {
    if (!tunePrompt.trim()) return;
    setTuneBusy(true);
    setTuneError("");
    setTuneResult(null);
    try {
      const r = await chatOnce({ model, prompt: tunePrompt, system, options });
      setTuneResult(r);
      setLastStats(r.stats);
    } catch (e: any) {
      setTuneError(String(e?.message ?? e));
    } finally {
      setTuneBusy(false);
    }
  }

  async function runBench() {
    const qs = benchQuestions.split("\n").map((q) => q.trim()).filter(Boolean);
    if (!qs.length || !benchModels.length) return;
    setBenchBusy(true);
    setBenchResults({});
    const results: Record<string, Record<number, BenchCell>> = {};
    // One model at a time; unload it after its last question so two 8B
    // models never sit in memory together (that is what caused the swap slowdown).
    for (const m of benchModels) {
      results[m] = {};
      for (let i = 0; i < qs.length; i++) {
        setBenchProgress(`${m} — question ${i + 1}/${qs.length}`);
        try {
          const r = await chatOnce({
            model: m,
            prompt: qs[i],
            system: benchUseTuning ? system : undefined,
            options: benchUseTuning ? options : undefined,
            keepAlive: i === qs.length - 1 ? 0 : "5m",
          });
          results[m][i] = { content: r.content, stats: r.stats };
        } catch (e: any) {
          results[m][i] = { content: "", error: String(e?.message ?? e) };
        }
        setBenchResults({ ...results });
      }
    }
    setBenchProgress("");
    setBenchBusy(false);
    try {
      await invoke("workshop_append", {
        file: "bench.jsonl",
        line: JSON.stringify({
          created: new Date().toISOString(),
          questions: qs,
          models: benchModels,
          usedTuning: benchUseTuning,
          options: benchUseTuning ? cleanOptions(options) : null,
          system: benchUseTuning ? system : null,
          results,
        }),
      });
    } catch {
      /* saving the log is best-effort */
    }
  }

  const modelfileText = useMemo(() => {
    const lines = [`FROM ${model}`, ""];
    for (const [k, v] of Object.entries(cleanOptions(options))) lines.push(`PARAMETER ${k} ${v}`);
    if (system.trim()) lines.push("", `SYSTEM """`, system.trim(), `"""`);
    return lines.join("\n");
  }, [model, options, system]);

  // ------------------------------------------------ surgery: automatic checks
  const checks = useMemo(() => {
    if (!show) return [] as { level: "ok" | "warn" | "bad"; text: string }[];
    const out: { level: "ok" | "warn" | "bad"; text: string }[] = [];
    const tpl: string = show.template ?? "";
    const params: string = show.parameters ?? "";
    const isLlama3 = /llama/i.test(show?.details?.family ?? "");
    if (tpl.trim() === "{{ .Prompt }}" || tpl.trim() === "") {
      out.push({ level: "bad", text: "Template is raw text ({{ .Prompt }}). The model gets no chat formatting and no end-of-turn signal — this is what made ri1-8b:latest write forum threads and never stop." });
    } else if (isLlama3 && !tpl.includes("<|start_header_id|>")) {
      out.push({ level: "warn", text: "Llama-family model without the Llama-3 chat markers in its template. It may follow instructions poorly." });
    } else {
      out.push({ level: "ok", text: "Chat template present — your message arrives as a proper 'user' turn." });
    }
    if (!/stop\s/.test(params)) {
      out.push({ level: tpl.includes("<|eot_id|>") ? "warn" : "bad", text: "No stop sequences set. The model relies on its own end-of-turn token; if it misses it, it runs to the length cap." });
    } else {
      out.push({ level: "ok", text: "Stop sequences set." });
    }
    const np = parseParameters(params).num_predict;
    if (np === undefined) out.push({ level: "warn", text: "No max-length cap (num_predict). A looping reply can run for a very long time." });
    const q: string = show?.details?.quantization_level ?? "";
    if (/Q8|F16|BF16/i.test(q)) {
      out.push({ level: "warn", text: `Weights are ${q} (large). Running a second 8B model alongside it can exhaust memory and slow everything to a crawl — unload one first.` });
    }
    if (lastStats && lastStats.doneReason === "length") {
      out.push({ level: "warn", text: "Your last Workshop reply hit the length cap instead of stopping on its own." });
    }
    return out;
  }, [show, lastStats]);

  // ------------------------------------------------ render
  const tabBtn = (id: typeof section, label: string) => (
    <button onClick={() => setSection(id)} disabled={section === id}>{label}</button>
  );

  return (
    <section style={{ borderTop: "1px solid #333", paddingTop: 12, display: "flex", flexDirection: "column", gap: 12, maxWidth: 1200 }}>
      <h3 style={{ margin: 0 }}>🛠 Workshop</h3>
      <p style={{ ...subtle, margin: 0 }}>
        Tune, test and inspect the local model. None of this changes the model's weights — it changes how the model is driven,
        and collects material for retraining. Files are saved in {workshopDir || "~/RI1-Workshop"}.
      </p>

      <div style={{ display: "flex", gap: 8, flexWrap: "wrap", alignItems: "center" }}>
        {tabBtn("tuning", "Tuning")}
        {tabBtn("bench", "Test bench")}
        {tabBtn("notebook", `Notebook (${notes.length})`)}
        {tabBtn("surgery", "Surgery")}
        <span style={{ flex: 1 }} />
        <label style={subtle}>Model&nbsp;</label>
        <select value={model} onChange={(e) => setModel(e.target.value)}>
          {!models.includes(model) && <option value={model}>{model}</option>}
          {models.map((m) => <option key={m} value={m}>{m}</option>)}
        </select>
        <button onClick={() => void refreshModels()} title="Reload the model list from Ollama">↻</button>
      </div>
      {modelsError && <div style={{ color: "#f88" }}>Ollama: {modelsError}</div>}

      {/* ------------------------------------------------ TUNING */}
      {section === "tuning" && (
        <div style={{ display: "grid", gridTemplateColumns: "minmax(280px, 380px) 1fr", gap: 12 }}>
          <div style={{ ...panel, display: "flex", flexDirection: "column", gap: 12 }}>
            <strong>Settings</strong>
            {SLIDERS.map((s) => {
              const changed = modelParams[s.key] !== undefined ? modelParams[s.key] !== options[s.key] : DEFAULT_OPTIONS[s.key] !== options[s.key];
              return (
                <div key={s.key}>
                  <div style={{ display: "flex", justifyContent: "space-between" }}>
                    <span>{s.label}{changed ? " •" : ""}</span>
                    <span style={{ fontFamily: "monospace" }}>{options[s.key]}</span>
                  </div>
                  <input
                    type="range"
                    min={s.min}
                    max={s.max}
                    step={s.step}
                    value={options[s.key]}
                    onChange={(e) => setOptions({ ...options, [s.key]: Number(e.target.value) })}
                    style={{ width: "100%" }}
                  />
                  <div style={subtle}>{s.help}</div>
                </div>
              );
            })}
            <div>
              <div>Seed</div>
              <input
                type="number"
                placeholder="random"
                value={options.seed ?? ""}
                onChange={(e) => setOptions({ ...options, seed: e.target.value === "" ? null : Number(e.target.value) })}
                style={{ width: 140 }}
              />
              <div style={subtle}>Fix a number to get the same reply for the same settings — useful for fair comparisons.</div>
            </div>
            <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
              <button onClick={() => { setOptions({ ...DEFAULT_OPTIONS, ...modelParams }); setSystem(modelSystem); }}>
                Reset to model's own settings
              </button>
              <button onClick={() => void navigator.clipboard?.writeText(modelfileText)} title="Copy these settings as a Modelfile you can save with `ollama create`">
                Copy as Modelfile
              </button>
            </div>
          </div>

          <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <div style={panel}>
              <div style={{ display: "flex", justifyContent: "space-between" }}>
                <strong>System prompt</strong>
                <span style={subtle}>{system === modelSystem ? "model's own" : "edited •"}</span>
              </div>
              <textarea value={system} onChange={(e) => setSystem(e.target.value)} rows={7} style={{ width: "100%", marginTop: 6 }} />
              <div style={subtle}>Standing instructions the model reads before every message. Edits apply only in the Workshop until you save them as a Modelfile.</div>
            </div>
            <div style={panel}>
              <strong>Try it</strong>
              <textarea
                value={tunePrompt}
                onChange={(e) => setTunePrompt(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) void runTuning(); }}
                placeholder="Ask something… (⌘+Enter to run)"
                rows={3}
                style={{ width: "100%", marginTop: 6 }}
              />
              <div style={{ display: "flex", gap: 8, alignItems: "center", marginTop: 6 }}>
                <button onClick={() => void runTuning()} disabled={tuneBusy || !tunePrompt.trim()}>{tuneBusy ? "Running…" : "Run"}</button>
                {tuneBusy && <span style={subtle}>The first run loads the model, which can take a while.</span>}
              </div>
              {tuneError && <div style={{ color: "#f88", marginTop: 8 }}>{tuneError}</div>}
              {tuneResult && (
                <div style={{ marginTop: 10, display: "flex", flexDirection: "column", gap: 6 }}>
                  <pre style={pre}>{tuneResult.content}</pre>
                  <StatsLine s={tuneResult.stats} />
                  <div style={{ display: "flex", gap: 8 }}>
                    <button onClick={() => void addNote({ question: tunePrompt, answer: tuneResult.content, source: "kept", model, options })}>
                      👍 Keep for notebook
                    </button>
                    <button onClick={() => { setEditing({ id: "", created: "", question: tunePrompt, answer: tuneResult.content, source: "corrected", model, options }); setSection("notebook"); }}>
                      ✏️ Correct it → notebook
                    </button>
                  </div>
                </div>
              )}
            </div>
          </div>
        </div>
      )}

      {/* ------------------------------------------------ BENCH */}
      {section === "bench" && (
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <div style={panel}>
            <strong>Questions (one per line)</strong>
            <textarea value={benchQuestions} onChange={(e) => setBenchQuestions(e.target.value)} rows={5} style={{ width: "100%", marginTop: 6 }} />
            <div style={{ marginTop: 8 }}>
              <strong>Models</strong> <span style={subtle}>(pick up to 3 — run one after another, unloaded between, to protect memory)</span>
              <div style={{ display: "flex", gap: 12, flexWrap: "wrap", marginTop: 4 }}>
                {models.map((m) => (
                  <label key={m} style={{ fontSize: 13 }}>
                    <input
                      type="checkbox"
                      checked={benchModels.includes(m)}
                      disabled={!benchModels.includes(m) && benchModels.length >= 3}
                      onChange={(e) => setBenchModels(e.target.checked ? [...benchModels, m] : benchModels.filter((x) => x !== m))}
                    /> {m}
                  </label>
                ))}
              </div>
            </div>
            <label style={{ display: "block", marginTop: 8, fontSize: 13 }}>
              <input type="checkbox" checked={benchUseTuning} onChange={(e) => setBenchUseTuning(e.target.checked)} /> Use the Tuning tab's settings and system prompt for every model
              <span style={subtle}> (off = each model uses its own built-in settings — the fair comparison)</span>
            </label>
            <div style={{ display: "flex", gap: 8, alignItems: "center", marginTop: 8 }}>
              <button onClick={() => void runBench()} disabled={benchBusy || !benchModels.length}>{benchBusy ? "Running…" : "Run test bench"}</button>
              {benchProgress && <span style={subtle}>{benchProgress}</span>}
            </div>
          </div>

          {Object.keys(benchResults).length > 0 && (
            <div style={{ overflowX: "auto" }}>
              <table style={{ borderCollapse: "collapse", width: "100%", tableLayout: "fixed" }}>
                <thead>
                  <tr>
                    <th style={{ textAlign: "left", width: 180, padding: 6, borderBottom: "1px solid #333" }}>Question</th>
                    {benchModels.map((m) => <th key={m} style={{ textAlign: "left", padding: 6, borderBottom: "1px solid #333" }}>{m}</th>)}
                  </tr>
                </thead>
                <tbody>
                  {benchQuestions.split("\n").map((q) => q.trim()).filter(Boolean).map((q, i) => (
                    <tr key={i}>
                      <td style={{ verticalAlign: "top", padding: 6, borderBottom: "1px solid #222", fontSize: 13 }}>{q}</td>
                      {benchModels.map((m) => {
                        const c = benchResults[m]?.[i];
                        return (
                          <td key={m} style={{ verticalAlign: "top", padding: 6, borderBottom: "1px solid #222" }}>
                            {!c ? <span style={subtle}>…</span> : c.error ? <span style={{ color: "#f88" }}>{c.error}</span> : (
                              <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                                <pre style={{ ...pre, maxHeight: 240 }}>{c.content}</pre>
                                <StatsLine s={c.stats} />
                                <div style={{ display: "flex", gap: 6 }}>
                                  <button onClick={() => void addNote({ question: q, answer: c.content, source: "kept", model: m })}>👍 Keep</button>
                                  <button onClick={() => { setEditing({ id: "", created: "", question: q, answer: c.content, source: "corrected", model: m }); setSection("notebook"); }}>✏️ Correct</button>
                                </div>
                              </div>
                            )}
                          </td>
                        );
                      })}
                    </tr>
                  ))}
                </tbody>
              </table>
              <div style={{ ...subtle, marginTop: 6 }}>Every run is logged to bench.jsonl in the Workshop folder.</div>
            </div>
          )}
        </div>
      )}

      {/* ------------------------------------------------ NOTEBOOK */}
      {section === "notebook" && (
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <div style={panel}>
            <strong>{editing?.id ? "Edit entry" : editing ? "Correct this answer" : "Add an entry"}</strong>
            <div style={subtle}>
              Write the answer the model <em>should</em> have given. These question→answer pairs are what RI1-8B was missing:
              a retraining run (LoRA) generally needs a few hundred good pairs before it reliably changes behaviour.
            </div>
            <NoteEditor
              key={editing ? editing.id || editing.question : "new"}
              initial={editing ?? { id: "", created: "", question: "", answer: "", source: "manual" }}
              onCancel={editing ? () => setEditing(null) : undefined}
              onSave={async (q, a) => {
                if (editing?.id) {
                  await saveAllNotes(notes.map((n) => (n.id === editing.id ? { ...n, question: q, answer: a } : n)));
                } else {
                  await addNote({ question: q, answer: a, source: editing?.source ?? "manual", model: editing?.model, options: editing?.options });
                }
                setEditing(null);
              }}
            />
          </div>
          {notesError && <div style={{ color: "#f88" }}>{notesError}</div>}
          <div style={subtle}>
            {notes.length} pair{notes.length === 1 ? "" : "s"} · {notes.filter((n) => n.source === "corrected").length} corrected ·{" "}
            {notes.filter((n) => n.source === "kept").length} kept as-is · {notes.filter((n) => n.source === "manual").length} written by hand
          </div>
          {[...notes].reverse().map((n) => (
            <div key={n.id} style={panel}>
              <div style={{ display: "flex", justifyContent: "space-between", gap: 8 }}>
                <strong style={{ fontSize: 14 }}>{n.question}</strong>
                <span style={subtle}>{n.source}{n.model ? ` · ${n.model}` : ""}</span>
              </div>
              <pre style={{ ...pre, marginTop: 6, maxHeight: 200 }}>{n.answer}</pre>
              <div style={{ display: "flex", gap: 8, marginTop: 6 }}>
                <button onClick={() => setEditing(n)}>Edit</button>
                <button onClick={() => { if (confirm("Remove this pair from the notebook?")) void saveAllNotes(notes.filter((x) => x.id !== n.id)); }}>Remove</button>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* ------------------------------------------------ SURGERY */}
      {section === "surgery" && (
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <div style={{ display: "flex", gap: 8 }}>
            <button onClick={() => { void loadModelSettings(model, false); void refreshPs(); }}>↻ Re-examine {model}</button>
          </div>
          {surgeryError && <div style={{ color: "#f88" }}>{surgeryError}</div>}

          <div style={panel}>
            <strong>Health checks</strong>
            <div style={{ display: "flex", flexDirection: "column", gap: 6, marginTop: 8 }}>
              {checks.map((c, i) => (
                <div key={i} style={{ display: "flex", gap: 8 }}>
                  <span>{c.level === "ok" ? "✅" : c.level === "warn" ? "⚠️" : "⛔"}</span>
                  <span style={{ fontSize: 13 }}>{c.text}</span>
                </div>
              ))}
              {!checks.length && <span style={subtle}>Loading…</span>}
            </div>
          </div>

          {show && (
            <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(320px, 1fr))", gap: 12 }}>
              <div style={panel}>
                <strong>Identity</strong>
                <table style={{ width: "100%", marginTop: 6, fontSize: 13 }}>
                  <tbody>
                    {Object.entries(show.details ?? {}).filter(([, v]) => v !== "" && v !== null && !(Array.isArray(v) && !v.length)).map(([k, v]) => (
                      <tr key={k} title={DETAIL_HELP[k] ?? ""}>
                        <td style={{ opacity: 0.7, paddingRight: 8, verticalAlign: "top" }}>{k}</td>
                        <td style={{ fontFamily: "monospace" }}>{Array.isArray(v) ? v.join(", ") : String(v)}</td>
                        <td style={{ ...subtle, paddingLeft: 8 }}>{DETAIL_HELP[k] ?? ""}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>

              <div style={panel}>
                <strong>Anatomy</strong>
                <table style={{ width: "100%", marginTop: 6, fontSize: 13 }}>
                  <tbody>
                    {Object.entries(show.model_info ?? {})
                      .filter(([k, v]) => typeof v !== "object" && Object.keys(MODEL_INFO_HELP).some((h) => k === h || k.endsWith("." + h)))
                      .map(([k, v]) => {
                        const help = Object.entries(MODEL_INFO_HELP).find(([h]) => k === h || k.endsWith("." + h))?.[1] ?? "";
                        return (
                          <tr key={k}>
                            <td style={{ opacity: 0.7, paddingRight: 8, verticalAlign: "top" }}>{k.replace(/^[a-z0-9]+\./, "")}</td>
                            <td style={{ fontFamily: "monospace", verticalAlign: "top" }}>{typeof v === "number" ? v.toLocaleString() : String(v)}</td>
                            <td style={{ ...subtle, paddingLeft: 8 }}>{help}</td>
                          </tr>
                        );
                      })}
                  </tbody>
                </table>
              </div>

              <div style={panel}>
                <strong>In memory right now</strong>
                <div style={subtle}>Models Ollama currently has loaded. Two large ones at once is what slowed things to one word every few seconds.</div>
                <table style={{ width: "100%", marginTop: 6, fontSize: 13 }}>
                  <tbody>
                    {(ps?.models ?? []).map((m: any) => (
                      <tr key={m.name}>
                        <td style={{ fontFamily: "monospace" }}>{m.name}</td>
                        <td>{fmtBytes(m.size)}</td>
                        <td style={subtle}>{m.size_vram ? `${Math.round((100 * m.size_vram) / m.size)}% on GPU` : "CPU"}</td>
                        <td style={subtle}>{m.expires_at ? `unloads ${new Date(m.expires_at).toLocaleTimeString()}` : ""}</td>
                      </tr>
                    ))}
                    {!(ps?.models ?? []).length && <tr><td style={subtle}>Nothing loaded.</td></tr>}
                  </tbody>
                </table>
              </div>

              <div style={panel}>
                <strong>Last Workshop run</strong>
                {lastStats ? (
                  <div style={{ display: "flex", flexDirection: "column", gap: 4, marginTop: 6, fontSize: 13 }}>
                    <span>Reply: {lastStats.replyTokens} tokens at {lastStats.tokensPerSec.toFixed(1)} tokens/sec</span>
                    <span>Read your prompt: {lastStats.promptTokens} tokens</span>
                    <span>Model load time: {fmtMs(lastStats.loadMs)} {lastStats.loadMs > 3000 ? "(it was not already in memory)" : ""}</span>
                    <span>Ended because: {lastStats.doneReason === "stop" ? "it finished on its own" : lastStats.doneReason === "length" ? "it hit the length cap" : lastStats.doneReason || "—"}</span>
                  </div>
                ) : <div style={subtle}>Run something in Tuning to see speed and stop behaviour here.</div>}
              </div>
            </div>
          )}

          {show && (
            <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(320px, 1fr))", gap: 12 }}>
              <div style={panel}>
                <strong>Template</strong>
                <div style={subtle}>The wrapper your message is placed in before the model sees it.</div>
                <pre style={{ ...pre, marginTop: 6 }}>{show.template || "(none)"}</pre>
              </div>
              <div style={panel}>
                <strong>Built-in parameters</strong>
                <div style={subtle}>Defaults baked into this model version (what the Tuning sliders start from).</div>
                <pre style={{ ...pre, marginTop: 6 }}>{show.parameters || "(none)"}</pre>
              </div>
              <div style={panel}>
                <strong>Built-in system prompt</strong>
                <pre style={{ ...pre, marginTop: 6 }}>{show.system || "(none)"}</pre>
              </div>
            </div>
          )}
          <div style={subtle}>
            Ollama reports a model's structure and settings, not what happens inside it while it thinks — this panel shows everything the runtime exposes.
          </div>
        </div>
      )}
    </section>
  );
}

function NoteEditor(props: {
  initial: NoteEntry;
  onSave: (question: string, answer: string) => void | Promise<void>;
  onCancel?: () => void;
}) {
  const [q, setQ] = useState(props.initial.question);
  const [a, setA] = useState(props.initial.answer);
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6, marginTop: 8 }}>
      <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Question" style={{ width: "100%" }} />
      <textarea value={a} onChange={(e) => setA(e.target.value)} placeholder="The answer it should give" rows={6} style={{ width: "100%" }} />
      <div style={{ display: "flex", gap: 8 }}>
        <button onClick={() => void props.onSave(q.trim(), a.trim())} disabled={!q.trim() || !a.trim()}>Save to notebook</button>
        {props.onCancel && <button onClick={props.onCancel}>Cancel</button>}
      </div>
    </div>
  );
}
