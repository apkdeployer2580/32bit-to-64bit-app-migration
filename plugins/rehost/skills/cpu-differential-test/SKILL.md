---
name: cpu-differential-test
description: Prove a hand-written ARM32 interpreter instruction by instruction against a reference core (Unicorn/QEMU). Catches the silent class of bug — a missing encoding that falls through and executes as a no-op, or almost-right flag effects — that shows up only as a black screen or a missing sprite. Use when adopting or forking a CPU core, when adding instruction encodings, or when rendering is wrong with no crash. 해석기 검증, 명령어 디코드 버그, 조용히 틀린 값.
---

# CPU 해석기를 명령어 단위로 증명하기

## 0. 왜 이것이 선택이 아닌가

해석기 버그는 **자기를 알리지 않는다.** 크래시도 어서션도 없이 약간 틀린 값을
내고, 그 값이 3만 명령 뒤에 포인터나 루프 한계로 흘러가서, 당신이 보는 것은
검은 화면이거나 안 그려진 스프라이트다. 증상에서 역추적하면 며칠이다.
`inst=0xe12fff37 blx r7 — r14 가 다릅니다` 에서 시작하면 몇 분이다.

그리고 상류 코어를 가져다 쓸 때 이 문제는 **거의 확실히** 발생한다. 대부분의
오픈소스 ARM 코어는 특정 세대(예: GBA = ARMv4T)를 겨냥해 만들어졌고, 당신의
게스트는 다른 세대다.

## 1. 도구

```bash
# 1) 당신의 해석기에 헬퍼 바이너리를 붙인다 (템플릿 제공)
cp ${CLAUDE_PLUGIN_ROOT}/scripts/armdiff_helper.rs <your_crate>/src/bin/armdiff.rs
# ADAPT 주석 4곳만 당신의 CPU 타입에 맞춘다
cargo build --release --bin armdiff

# 2) 돌린다
python ${CLAUDE_PLUGIN_ROOT}/scripts/armdiff.py \
    --helper target/release/armdiff --n 20000 --class all
```

무작위 명령어 + 무작위 레지스터/메모리 상태를 만들어 **한 명령**을 양쪽에서
실행하고 레지스터·플래그·메모리를 전부 비교한다. 클래스:

`smul` `ldrd` `half` `dataproc` `mul` `mullong` `block` `single` `branchex`

프로토콜은 stdin/stdout 바이너리 한 장이다(자세히는 `armdiff.py` 상단 독스트링).
해석기가 Rust 가 아니어도 그 프로토콜만 맞추면 된다.

## 2. 실제로 잡히는 버그의 모양

아래는 전부 **동작하는 것처럼 보이는 빌드**에서 나온 실제 사례다.

### (a) 인코딩을 아예 놓쳐서 다른 패턴에 매치된다 — 최악

`BLX <reg>` 와 `BX <reg>` 는 bits 7..4 가 `0011` / `0001` 로만 다르다. 상류
디코드 패턴이 bit 5 를 0으로 고정해 두면 `bx` 만 잡고 `blx` 를 놓친다. 놓친
인코딩은 아래로 흘러 데이터처리 패턴에 매치되고, 하필 **opcode 9(TEQ) + S=0**,
즉 레지스터도 플래그도 안 바꾸는 **완전한 no-op** 으로 실행된다.

결과: **모든 간접 호출이 조용히 사라진다.** 화면에는 함수 포인터로 그리는
것(스프라이트·타일·일러스트)만 전부 빠지고 HUD/폰트는 남는다. 패치를 다 꺼도
같으므로 렌더러는 무죄고, 진짜 단서는 디버그 빌드가 뱉은 `assertion failed:
rs <= 14` 하나였다 — `Rs=R15` 는 컴파일러가 낼 수 없는 형태라 **오디코드
신호**다. 릴리스 빌드에는 어서션이 없어 조용히 넘어간다.

**교훈**: 디버그 빌드의 어서션을 지우지 마라. 그리고 `branchex` 클래스를 꼭 돌려라.

### (b) 세대 차이로 없는 명령어군

ARMv5TE 에 있고 ARMv4T 에 없는 것: `SMUL<x><y>` / `SMLA` / `SMLAW` / `SMLAL`
계열, `LDRD`/`STRD`. GCC 는 이것들을 자주 낸다(실측: SMUL 계열 66곳,
LDRD/STRD 약 290개). 상류가 `L==0, S==1` 조합을 "있을 수 없음"으로 단정해 두는
경우도 있다.

또 하나: **`cond == 0b1111`** 은 ARMv5 에서 조건이 아니라 **무조건 명령 영역**이다.
조건 코드로 처리하면 안 된다.

### (c) 플래그가 거의 맞는 것 — 파급이 가장 넓다

* `MUL/MLA/SMULL/UMULL/SMLAL` + S 비트: ARMv5 는 **C·V 를 보존한다.** 지우면 안 된다.
* **즉시값 회전이 0인 데이터처리**(`ands r0,r0,#1`, `movs`, `bics`, `tst` …):
  회전이 0이면 캐리는 **건드리지 않는 것이 맞다.** 덮어쓰면 GCC 가 가장 많이
  뿜는 형태라 파급이 엄청나다.

## 3. 하네스 자체의 함정 — 두 개, 둘 다 겪는다

이 두 가지는 **당신의 해석기가 아니라 비교 도구가 틀린 경우**다. 모르면
멀쩡한 해석기를 몇 시간 판다. (제공된 `armdiff.py` 는 둘 다 이미 고쳐져 있다.)

**1) 레퍼런스의 레지스터 상수는 연속이 아니다.**
Unicorn 에서 `UC_ARM_REG_R0 + i` 는 R0..R12 까지만 맞는다. R13/R14/R15 는
전혀 다른 값(12/10/11)이라, 더하기로 접근하면 **엉뚱한 레지스터 세 개**를 읽는다.
SP/LR/PC 를 쓰는 명령을 테스트하기 전까지는 보이지 않고, 그 순간
**모든 `blx <reg>` 케이스가 거짓 불일치**를 낸다.

**2) CPSR 을 레지스터보다 나중에 쓰면 SP/LR 이 날아간다.**
SP 와 LR 은 프로세서 모드별로 뱅킹된다. 코어의 초기 모드에서 SP/LR 을 써 넣고
그 다음 CPSR 을 User 로 바꾸면 User 뱅크가 들어오면서 값이 버려진다. 레퍼런스
쪽 r13/r14 가 전부 0으로 나온다. **CPSR 을 먼저, 레지스터를 나중에.**

## 4. 비교에서 제외해야 하는 것

* **r15(PC)** — 파이프라인 규약이 코어마다 다르다. 기본은 제외하고, 분기
  테스트에서만 따로 본다(`--check-pc`).
* **CPSR 하위 비트** — 상위 4비트(NZCV)만 비교한다. 나머지에는 이 게스트가 절대
  안 건드리는 비트가 섞여 있다.
* **레퍼런스가 거부한 인코딩** — 비교 대상이 아니다. 조용히 건너뛰고 `comparable`
  개수로 보고한다.
* **UNPREDICTABLE 인코딩** — 생성기가 처음부터 만들지 않게 한다(정렬·레지스터
  중복 제약). 여기서 나는 불일치는 아무것도 증명하지 않는다.
* **세대 밖 인코딩** — 예: 데이터처리 opcode 8..11 + S=0 은 ARMv5 에서 MSR/MRS
  공간이지만 ARMv6T2+ 에서는 MOVW/MOVT 다. 레퍼런스가 v7 코어면 **두 아키텍처를
  비교하는 셈**이라 의미가 없다. 생성기에서 빼라.

## 5. 디코드 LUT 를 넣었다면 등가성 테스트를 같이 넣어라

명령어마다 20번씩 mask-compare 하는 선형 스캔은 해석기에서 가장 비싼 것 중
하나다. LUT 로 바꾸는 것은 옳지만, **LUT 와 원래 스캔이 모든 관련 비트 조합에서
같은 답을 내는지** 확인하는 테스트를 반드시 같이 넣는다.

```
cargo test --release   #  decode_lut_matches_linear
```

이 테스트는 디코드에 관여하는 비트 전 범위를 훑는다. 몇 초면 끝나고, LUT 를
손댈 때마다 돈다.

## 6. 포크했다면 상류와 다른 곳을 표시하라

상류에서 갈라진 줄마다 고정된 마커 주석을 단다:

```rust
// ★ <project> fork: ARMv5TE preserves C and V here; upstream (v4T) clears them.
```

`grep` 한 번으로 "우리가 뭘 바꿨나"가 나오고, 상류를 다시 당길 때 충돌 지점이
그대로 목록이 된다.

## 7. 언제 돌리는가

* 해석기를 **채택/포크한 직후** — 한 번, 전 클래스.
* 인코딩을 **추가·수정할 때마다** — 그 클래스.
* **"크래시는 없는데 화면이 이상하다"** 가 나올 때마다 — 다른 걸 보기 전에.
  0 mismatch 를 확인하고 나면 그 다음부터는 해석기를 의심하지 않아도 된다.
  그 확신이 이 도구의 진짜 값어치다.
