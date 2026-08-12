# 32bit → 64bit app migration

A Claude Code plugin marketplace for **rehosting** a 32-bit ARM game or app
inside a 64-bit process.

32비트 ARM 게임/앱을 **64비트 프로세스 안에서 원본 그대로 실행**시키는 작업의
방법론을 Claude Code 스킬로 묶은 마켓플레이스.

---

## The premise / 전제

There is no tool that turns ARM32 machine code into ARM64 machine code. If the
app's body is a 32-bit `.so` and you don't have the source, "converting the APK
to 64-bit" is not a thing that exists.

What does exist: run the original 32-bit code on an **interpreter inside a pure
64-bit process**, and natively implement only the **narrow seam** where that
code meets the outside world. The game logic stays the shipped binary, bit for
bit; everything around it is 64-bit.

> **32비트 기계어를 64비트 기계어로 변환하는 도구는 없다.** 대신 원본 코드를
> 64비트 프로세스 안에서 **해석 실행**하고, 그 코드가 바깥과 만나는 **좁은
> 경계면만** 네이티브로 구현한다. 이 마켓플레이스는 그 작업의 순서, 함정,
> 그리고 **증명 방법**을 담는다.

Whether it is worth doing is one measurable number — how many distinct things
the binary calls *outside itself*. The first skill measures it for you.

## Install

```
/plugin marketplace add apkdeployer2580/32bit-to-64bit-app-migration
/plugin install rehost@32bit-to-64bit-app-migration
```

Then just describe what you are doing — the skills trigger on their own. Or
start explicitly:

```
/rehost:start payload/libgame.so
```

## What's in it

| Skill | When it fires |
|---|---|
| **rehost-feasibility** | "can this 32-bit game be ported", start of a project — measures the seam and writes the go/no-go |
| **rehost-bringup** | guest address space, ELF32 loader, import/JNI trampolines, first frame, black-screen traps |
| **deterministic-probe** | the headless replay harness: virtual clock, scripted input, frame dumps, PC profiling — the development loop |
| **cpu-differential-test** | prove the interpreter instruction-by-instruction against Unicorn; the silent-decode-bug class |
| **frame-optimization** | slow, stutters, lags after a boss fight — profile to a named guest function, port it to the host bit-exactly, prove it pixel-identical |
| **guest-behavior-patching** | fix an original-game bug or add a QoL change through host-side hooks instead of editing the binary |
| **legacy-assets-and-encoding** | assets won't load, fonts crash at boot, text renders as wrong-but-readable characters |
| **network-rehost** | online features hang; the original servers are gone; decode the wire protocol and stand up a private server |
| **ship-64bit-android** | release signing, update compatibility, **not destroying players' saves**, lifecycle and surface handling |

### Scripts (`plugins/rehost/scripts/`)

All game-agnostic, all runnable on their own.

| Script | What it does |
|---|---|
| `seam_census.py` | the go/no-go report: imports by library, JNI entry points, symbol survival, arch tags, relocation histogram, loader hazards |
| `xref.py` | every branch **and every stored 32-bit word** pointing at a symbol — finds functions reached only through a pointer, which a `bl` search reports as dead code |
| `disasm.py` | symbol-aware disassembly with PIC/GOT globals resolved to names and incoming stack-argument offsets computed from the prologue |
| `armdiff.py` + `armdiff_helper.rs` | differential-test your interpreter against Unicorn, one instruction at a time |
| `ab_compare.py` | pixel-exact frame-dump comparison — the acceptance test for every host port |
| `frame_stats.py` | per-frame cost as a distribution (p90/p99/max/over-budget/worst frames), with before-vs-after |

```bash
python plugins/rehost/scripts/seam_census.py libgame.so
python plugins/rehost/scripts/xref.py libgame.so SomeBlitter --words
python plugins/rehost/scripts/disasm.py libgame.so SomeBlitter --frame
python plugins/rehost/scripts/ab_compare.py out_ref out_test
```

Dependencies: `pip install pyelftools capstone pillow unicorn`
(each script only needs the ones it names).

## Where this came from

Distilled from two shipped rehosts of Korean feature-phone (WIPI) RPGs onto
64-bit-only Android — one native-C guest, one JVM guest. Everything specific to
those games (addresses, symbol names, struct offsets, protocol opcodes) has
been left out; what remains is the part that transferred between them.

Some numbers those projects produced, as a sense of what the method is worth:

* interpreter-only → all hot guest functions hosted: **15.1 billion → 72 million**
  guest instructions over the same 2600-frame deterministic replay (**≈210×**),
  with all 12 dumped frames **pixel-identical** to the unpatched interpreter
* frames over the 33 ms budget: **205/2600 → 1/2600**. The mean was never the
  problem — it sat at a comfortable 10.6 ms the whole time, while p99 was 84.6 ms
* one menu-open frame: **55.6 million guest instructions → O(w·h)**, identical output
* the seam that made all of it possible: **79 imported symbols, 14 JNI entry
  points** against 878 KB of machine code nobody had the source to

The skills are written in Korean with English frontmatter, matching the
working notes they were extracted from.

## License

MIT — see [LICENSE](LICENSE).
