#!/usr/bin/env node
"use strict";

const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const SAMPLE_RATE = 44100;
const BLOCK_SIZE = 64;
const DEFAULT_FRAMES = 15000;

function parseArgs(argv) {
  let input = null;
  let frames = DEFAULT_FRAMES;
  const importDirs = [];
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "-n") {
      i += 1;
      if (i >= argv.length) throw new Error("-n requires a frame count");
      frames = Number.parseInt(argv[i], 10);
      if (!Number.isFinite(frames) || frames < 0) throw new Error(`invalid frame count: ${argv[i]}`);
    } else if (arg === "-I") {
      i += 1;
      if (i >= argv.length) throw new Error("-I requires a directory");
      importDirs.push(argv[i]);
    } else if (arg === "-double" || arg === "-single") {
      // Accepted for parity with the other impulse runners. The AssemblyScript
      // backend exposes an f32 audio interface either way.
    } else if (arg.startsWith("-")) {
      throw new Error(`unknown option: ${arg}`);
    } else if (input === null) {
      input = arg;
    } else {
      throw new Error(`unexpected argument: ${arg}`);
    }
  }
  if (input === null) throw new Error("missing DSP input");
  return { input, frames, importDirs };
}

function normalize(value) {
  if (Math.abs(value) < 1e-6) return 0;
  return value;
}

function formatHeader(inputs, outputs, frames) {
  return [
    `number_of_inputs  : ${String(inputs).padStart(3, " ")}`,
    `number_of_outputs : ${String(outputs).padStart(3, " ")}`,
    `number_of_frames  : ${String(frames).padStart(6, " ")}`,
  ];
}

function formatFrame(index, values) {
  return `${String(index).padStart(6, " ")} : ${values.map((value) => ` ${Number(value).toFixed(6)}`).join(" ")}`;
}

function collectItems(ui, out = []) {
  if (!Array.isArray(ui)) return out;
  for (const item of ui) {
    if (item && Array.isArray(item.items)) {
      collectItems(item.items, out);
    } else if (item) {
      out.push(item);
    }
  }
  return out;
}

function compileAsc(faustRs, input, importDirs, tmpDir) {
  const ascPath = path.join(tmpDir, `${path.basename(input, ".dsp")}.ts`);
  const args = ["-lang", "asc", "--json", input, "-o", ascPath];
  for (const dir of importDirs) args.push("-I", dir);
  const result = spawnSync(faustRs, args, { encoding: "utf8" });
  if (result.status !== 0) {
    const detail = (result.stderr || result.stdout || "").trim();
    throw new Error(detail || `failed to compile ${input} to AssemblyScript`);
  }
  return { ascPath, jsonPath: ascPath.replace(/\.ts$/i, ".json") };
}

function wrapperSource(source, json) {
  const classMatch = source.match(/export\s+class\s+([A-Za-z_][A-Za-z0-9_]*)/);
  if (!classMatch) throw new Error("generated AssemblyScript class not found");
  const className = classMatch[1];
  const buttons = collectItems(json.ui)
    .filter((item) => item.type === "button" && /^[A-Za-z_][A-Za-z0-9_]*$/.test(item.varname || ""))
    .map((item) => item.varname);
  const buttonAssignments = buttons
    .map((name) => `  dsp.${name} = value;`)
    .join("\n");
  return `${source}

let dsp: ${className} = new ${className}();
let inputsBuf: Array<StaticArray<f32>> = new Array<StaticArray<f32>>(0);
let outputsBuf: Array<StaticArray<f32>> = new Array<StaticArray<f32>>(0);

export function setup(size: i32): void {
  inputsBuf = new Array<StaticArray<f32>>(dsp.getNumInputs());
  outputsBuf = new Array<StaticArray<f32>>(dsp.getNumOutputs());
  for (let i: i32 = 0; i < dsp.getNumInputs(); i = i + 1) {
    inputsBuf[i] = new StaticArray<f32>(size);
  }
  for (let i: i32 = 0; i < dsp.getNumOutputs(); i = i + 1) {
    outputsBuf[i] = new StaticArray<f32>(size);
  }
}

export function init(sampleRate: i32): void {
  dsp.init(sampleRate);
}

export function getNumInputs(): i32 {
  return dsp.getNumInputs();
}

export function getNumOutputs(): i32 {
  return dsp.getNumOutputs();
}

export function setInput(channel: i32, frame: i32, value: f32): void {
  inputsBuf[channel][frame] = value;
}

export function getOutput(channel: i32, frame: i32): f32 {
  return outputsBuf[channel][frame];
}

export function setButtons(value: f32): void {
${buttonAssignments}
}

export function compute(count: i32): void {
  dsp.compute(count, inputsBuf, outputsBuf);
}
`;
}

function compileWrapper(ascBin, ascPath, wasmPath) {
  const result = spawnSync(
    ascBin,
    [ascPath, "--target", "release", "--exportRuntime", "--outFile", wasmPath],
    { encoding: "utf8" },
  );
  if (result.status !== 0) {
    const detail = (result.stderr || result.stdout || "").trim();
    throw new Error(detail || `failed to compile ${ascPath} with asc`);
  }
}

async function run() {
  const { input, frames, importDirs } = parseArgs(process.argv.slice(2));
  const faustRs = process.env.FAUST_RS || path.join("..", "..", "target", "release", "faust-rs");
  const ascBin = process.env.ASC || "asc";
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "faust-rs-impulse-asc-"));
  try {
    const { ascPath, jsonPath } = compileAsc(faustRs, input, importDirs, tmpDir);
    const json = JSON.parse(fs.readFileSync(jsonPath, "utf8"));
    const wrappedPath = path.join(tmpDir, "wrapped.ts");
    const wasmPath = path.join(tmpDir, "wrapped.wasm");
    fs.writeFileSync(wrappedPath, wrapperSource(fs.readFileSync(ascPath, "utf8"), json));
    compileWrapper(ascBin, wrappedPath, wasmPath);

    const instance = await WebAssembly.instantiate(fs.readFileSync(wasmPath), {
      env: {
        abort(_msg, _file, line, col) {
          throw new Error(`AssemblyScript abort at ${line}:${col}`);
        },
      },
    });
    const exp = instance.instance.exports;
    exp.setup(BLOCK_SIZE);
    exp.init(SAMPLE_RATE);
    const inputs = exp.getNumInputs();
    const outputs = exp.getNumOutputs();
    const lines = formatHeader(inputs, outputs, frames);

    let written = 0;
    let cycle = 0;
    while (written < frames) {
      const n = Math.min(BLOCK_SIZE, frames - written);
      for (let channel = 0; channel < inputs; channel += 1) {
        for (let frame = 0; frame < BLOCK_SIZE; frame += 1) {
          exp.setInput(channel, frame, 0.0);
        }
      }
      if (written === 0) {
        for (let channel = 0; channel < inputs; channel += 1) {
          exp.setInput(channel, 0, 1.0);
        }
      }
      exp.setButtons(cycle === 0 ? 1.0 : 0.0);
      exp.compute(n);
      for (let frame = 0; frame < n; frame += 1) {
        const values = [];
        for (let channel = 0; channel < outputs; channel += 1) {
          values.push(normalize(Number(exp.getOutput(channel, frame))));
        }
        lines.push(formatFrame(written, values));
        written += 1;
      }
      cycle += 1;
    }
    process.stdout.write(lines.join("\n"));
    if (lines.length > 0) process.stdout.write("\n");
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
}

run().catch((error) => {
  console.error(error && error.stack ? error.stack : String(error));
  process.exit(1);
});
