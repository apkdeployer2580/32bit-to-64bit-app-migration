---
description: Start a 32-bit → 64-bit rehosting project — run the seam census on the target binary and produce a go/no-go with a plan.
argument-hint: <path to .so or .apk>
---

Start a rehosting assessment for `$1`.

1. Load the `rehost-feasibility` skill and follow it.
2. Run the seam census on `$1` (extract the 32-bit libraries first if it is an
   APK) and report the numbers: imports, JNI entry points, symbol survival,
   architecture tags, relocation histogram, hazards.
3. Count the reverse direction too — the platform callbacks the binary reaches
   back into — because that is not in the ELF.
4. Check how the guest draws: if the GL imports are few and there is one
   `glDrawArrays` caller, say so; it changes the whole plan.
5. Give a go/no-go with the reasoning, then a concrete phase plan naming which
   of these skills covers each phase:
   `rehost-bringup` → `deterministic-probe` → `cpu-differential-test` →
   `frame-optimization` → `legacy-assets-and-encoding` /
   `guest-behavior-patching` / `network-rehost` → `ship-64bit-android`.

Do not start writing runtime code in this command. The deliverable is the
assessment and the plan.
