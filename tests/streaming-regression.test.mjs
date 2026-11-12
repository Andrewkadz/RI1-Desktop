// Place this file in tests/ and run:
// node --test tests/streaming-regression.test.mjs
//
// Uses Node's built-in modules. Exercises the production listener, registration,
// error, Stop, and Retry code with mocked Tauri events and React state setters.
// These checks do not launch a Tauri window, model, or Rust subprocess.
// Source markers deliberately fail loudly if the handlers are refactored.

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import vm from "node:vm";

const sourcePath = process.env.RI1_DESKTOP_ROOT
  ? join(process.env.RI1_DESKTOP_ROOT, "src", "App.tsx")
  : new URL("../src/App.tsx", import.meta.url);
const source = readFileSync(sourcePath, "utf8");

function position(marker, from = 0) {
  const index = source.indexOf(marker, from);
  assert.notEqual(index, -1, "Production source marker missing: " + marker);
  return index;
}

// Remove only the TypeScript syntax in the extracted fragments. Runtime handler
// logic comes directly from App.tsx rather than a separate model of the behavior.
function executable(code) {
  return code
    .replaceAll("e.payload as any", "e.payload")
    .replaceAll("catch (e: any)", "catch (e)")
    .replaceAll("catch (ee: any)", "catch (ee)")
    .replaceAll("invoke<string>", "invoke")
    .replaceAll("listen<{ chunk: string; done: boolean }>", "listen");
}

const listenMarker =
  "await listen<{ chunk: string; done: boolean }>(topic, (e) => {";
const listenStart = position(listenMarker);
const statementStart = source.lastIndexOf("\n", listenStart) + 1;
const callbackStart = listenStart + listenMarker.length;
const callbackEnd = position("\n      });", callbackStart);
const backendStart = position("// Kick off streaming on backend", callbackEnd);
const catchStart = position("    } catch (e: any) {", backendStart);
const sendEnd = position("\n  }\n\n  async function runKosmos", catchStart);

const callbackCode = executable(source.slice(callbackStart, callbackEnd));
const registrationCode = executable(source.slice(statementStart, backendStart));
const errorCode = executable(source.slice(catchStart, sendEnd));
const stopStart = position("  async function stopGeneration() {");
const stopEnd = position("\n  function lastUserMessage()", stopStart);
const stopCode = source.slice(stopStart, stopEnd);
const retryStart = position("  function retryLast() {");
const retryEnd = position("\n  function editAndResend()", retryStart);
const retryCode = source.slice(retryStart, retryEnd);

function deferred() {
  let resolve;
  const promise = new Promise(accept => {
    resolve = accept;
  });
  return { promise, resolve };
}

function createHarness(useRi1 = false) {
  const ui = {
    loading: true,
    currentStream: {
      id: "stream-A",
      backend: useRi1 ? "ri1" : "ollama",
    },
    messages: [{ id: "assistant-A", text: "" }],
    errors: [],
    disposals: [],
    calls: [],
  };
  const listenerA = () => ui.disposals.push("A");
  const env = {
    activeRequestId: { current: "request-A" },
    requestId: "request-A",
    streamId: "stream-A",
    topic: "stream-A",
    // A retained render's value stays fixed while the ref remains mutable.
    currentStream: ui.currentStream,
    assistantId: "assistant-A",
    activeThreadId: "thread-A",
    useRi1,
    ttsEnabled: false,
    model: "test-model",
    fullPrompt: "Request A",
    disposer: listenerA,
    unlistenRef: { current: listenerA },
    setLoading(value) {
      ui.loading = value;
    },
    setCurrentStream(value) {
      ui.currentStream = value;
    },
    updateActiveThreadMessages(update) {
      ui.messages = update(ui.messages);
    },
    setError(error) {
      ui.errors.push(error);
    },
    setPrompt() {},
    setLogs() {},
    ingestProtocolIfPresent() {},
    extractProtocolsFromText() {},
    updateKnowledgeGraph() {},
    speakText() {},
    async invoke(name) {
      ui.calls.push(name);
      return "Fallback output";
    },
  };
  return { ui, env, listenerA };
}

function createListener(harness) {
  return vm.runInNewContext(
    "(function () { let assistantText = ''; return (e) => {" +
      callbackCode +
      "\n}; })()",
    harness.env,
  );
}

function rejectRequest(harness) {
  harness.env.reviewError = new Error("Process exited unsuccessfully");
  return vm.runInNewContext(
    "(async function () { try { throw reviewError;\n" + errorCode + "\n})()",
    harness.env,
  );
}

function stopRequest(harness) {
  return vm.runInNewContext(stopCode + "\nstopGeneration()", harness.env);
}

function registerListener(harness) {
  return vm.runInNewContext(
    "(async function () { let assistantText = '';\n" +
      registrationCode +
      "\nreturn 'backend-ready'; })()",
    harness.env,
  );
}

function startRequestB(harness) {
  harness.env.activeRequestId.current = "request-B";
  harness.ui.currentStream = { id: "stream-B", backend: "ollama" };
  harness.ui.loading = true;
  const listenerB = () => harness.ui.disposals.push("B");
  harness.env.unlistenRef.current = listenerB;
  return listenerB;
}

function assertRequestBIntact(harness, listenerB) {
  assert.equal(harness.env.activeRequestId.current, "request-B");
  assert.equal(harness.ui.currentStream?.id, "stream-B");
  assert.equal(harness.ui.loading, true);
  assert.equal(harness.env.unlistenRef.current, listenerB);
  assert.equal(harness.ui.disposals.includes("B"), false);
}

test("valid reply chunks and completion update the active request", () => {
  const harness = createHarness();
  const emit = createListener(harness);

  emit({ payload: { chunk: "Reply text", done: false } });
  emit({ payload: { chunk: "", done: true } });

  assert.equal(harness.ui.messages[0].text, "Reply text");
  assert.equal(harness.ui.loading, false);
  assert.equal(harness.ui.currentStream, null);
  assert.equal(harness.env.activeRequestId.current, null);
  assert.deepEqual(harness.ui.disposals, ["A"]);
});

test("older reply and completion events preserve the newer request", () => {
  const harness = createHarness();
  const emit = createListener(harness);
  const listenerB = startRequestB(harness);

  emit({ payload: { chunk: "Stale reply", done: false } });
  emit({ payload: { chunk: "", done: true } });

  assertRequestBIntact(harness, listenerB);
  assert.equal(harness.ui.messages[0].text, "");
});

test("A rejection after B starts disposes A and preserves B", async () => {
  const harness = createHarness();
  const listenerB = startRequestB(harness);

  await rejectRequest(harness);

  assertRequestBIntact(harness, listenerB);
  assert.equal(harness.ui.calls.length, 0);
});

test("a delayed Stop acknowledgement preserves a newer request", async () => {
  const harness = createHarness();
  const cancellation = deferred();
  harness.env.invoke = async name => {
    harness.ui.calls.push(name);
    return cancellation.promise;
  };

  const stopped = stopRequest(harness);
  const listenerB = startRequestB(harness);
  cancellation.resolve();
  await stopped;

  assertRequestBIntact(harness, listenerB);
  assert.deepEqual(harness.ui.calls, ["cancel_stream"]);
});

test("two pending Stop calls resolving around B preserve B", async () => {
  const harness = createHarness();
  const firstCancellation = deferred();
  const secondCancellation = deferred();
  const cancellations = [firstCancellation, secondCancellation];
  harness.env.invoke = async name => {
    const pending = cancellations[harness.ui.calls.length];
    harness.ui.calls.push(name);
    return pending.promise;
  };

  const firstStop = stopRequest(harness);
  const secondStop = stopRequest(harness);
  secondCancellation.resolve();
  await secondStop;

  const listenerB = startRequestB(harness);
  firstCancellation.resolve();
  await firstStop;

  assertRequestBIntact(harness, listenerB);
});

test("late A registration disposes A and preserves B's listener", async () => {
  const harness = createHarness();
  const registration = deferred();
  harness.env.disposer = null;
  harness.env.listen = async () => registration.promise;

  const pending = registerListener(harness);
  const listenerB = startRequestB(harness);
  registration.resolve(harness.listenerA);
  await pending;

  assertRequestBIntact(harness, listenerB);
  assert.deepEqual(harness.ui.disposals, ["A"]);
});

test("late A fallback cannot update messages or clear B", async () => {
  const harness = createHarness();
  const fallback = deferred();
  harness.env.invoke = async name => {
    harness.ui.calls.push(name);
    return fallback.promise;
  };

  const pending = rejectRequest(harness);
  assert.deepEqual(harness.ui.calls, ["gen_ollama"]);
  const listenerB = startRequestB(harness);
  fallback.resolve("Late fallback output");
  await pending;

  assertRequestBIntact(harness, listenerB);
  assert.equal(harness.ui.messages[0].text, "");
});

test("an active RI1 error is shown while partial output is retained", async () => {
  const harness = createHarness(true);
  createListener(harness)({
    payload: { chunk: "Partial reply", done: false },
  });

  await rejectRequest(harness);

  assert.equal(harness.ui.errors.length, 1);
  assert.match(harness.ui.errors[0], /Process exited unsuccessfully/);
  assert.equal(harness.ui.messages[0].text, "Partial reply");
  assert.equal(harness.ui.loading, false);
  assert.equal(harness.env.activeRequestId.current, null);
  assert.equal(harness.ui.calls.length, 0);
});

test("cancellation during listener setup disposes the abandoned listener", async () => {
  const harness = createHarness();
  const registration = deferred();
  harness.env.disposer = null;
  harness.env.listen = async () => registration.promise;
  const pending = registerListener(harness);

  harness.env.activeRequestId.current = null;
  harness.env.unlistenRef.current = null;
  harness.ui.currentStream = null;
  harness.ui.loading = false;
  registration.resolve(harness.listenerA);

  assert.equal(await pending, undefined);
  assert.deepEqual(harness.ui.disposals, ["A"]);
  assert.equal(harness.ui.calls.length, 0);
});

test("Retry passes the selected message while input contains different text", () => {
  const submitted = [];
  vm.runInNewContext(retryCode + "\nretryLast()", {
    prompt: "Different text currently in the input",
    lastUserMessage: () => ({ text: "Retry Φπε request" }),
    send: async text => {
      submitted.push(text);
    },
  });

  assert.deepEqual(submitted, ["Retry Φπε request"]);
});

