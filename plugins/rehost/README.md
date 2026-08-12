# rehost

Run an unmodified 32-bit ARM binary inside a 64-bit process.

Nine skills covering the whole arc — feasibility, bring-up, the deterministic
development loop, interpreter correctness, frame-budget optimisation, behaviour
patching, assets and legacy encodings, private-server revival, and shipping —
plus six game-agnostic analysis scripts.

See the [marketplace README](../../README.md) for the full table and install
instructions.

## Layout

```
skills/
  rehost-feasibility/           can this be done, and what is the evidence
  rehost-bringup/               loader, memory map, HLE seam, first frame
  deterministic-probe/          headless replay: the development loop
  cpu-differential-test/        prove the interpreter against a reference core
  frame-optimization/           profile -> host port -> pixel-identical A/B
    references/
      porting-a-guest-function.md
      pixel-loop-anatomy.md
  guest-behavior-patching/      deliberate changes, host-side hooks
  legacy-assets-and-encoding/   archives, name<->id registries, EUC-KR/Shift-JIS
  network-rehost/               dead servers, wire protocols, private server
  ship-64bit-android/           signing, saves, lifecycle, surfaces
commands/
  start.md                      /rehost:start <binary>
scripts/
  seam_census.py  xref.py  disasm.py
  armdiff.py  armdiff_helper.rs
  ab_compare.py  frame_stats.py
```

Skills reference the scripts as `${CLAUDE_PLUGIN_ROOT}/scripts/<name>.py`.
