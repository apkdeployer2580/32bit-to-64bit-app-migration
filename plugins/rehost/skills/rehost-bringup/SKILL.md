---
name: rehost-bringup
description: Bring a 32-bit ARM guest binary up inside a 64-bit host — guest address space, ELF32 loader and relocations, import/JNI trampolines, the platform lifecycle, and the first rendered frame. Covers the traps that produce a black screen or a silent hang. Use when starting to implement the runtime, wiring the HLE layer, loading the .so, or when the guest boots to nothing. 게스트 로더/메모리맵/HLE 구현, 첫 프레임, 검은 화면.
---

# 브링업 — 게스트를 64비트 프로세스 안에서 처음 돌리기

`rehost-feasibility` 가 "가능하다"를 냈다는 전제. 여기서는 **첫 프레임이 나올
때까지**의 구조와 함정만 다룬다.

## 0. 크레이트 구조를 먼저 고정한다

```
  android_app (arm64-v8a APK)              probe (헤드리스, PC)
                    \                     /
                     +----- core --------+
                     |  Vm: CPU·메모리   |
                     |  libc/GL/JNI HLE  |
                     +---------+---------+
                               | ARM32 해석
                        libgame.so  (원본 그대로)
```

**세 개다. 두 개가 아니다.**

* `core` — 런타임 전부. 플랫폼 의존성 없음.
* `android` (또는 다른 실기기 셸) — 창·서피스·입력·오디오 장치.
* `probe` — 헤드리스 러너. **같은 `core` 를 쓴다.**

`probe` 를 나중에 만들면 안 된다. 첫날부터 있어야 한다 — 기기 없이, 창 없이,
결정적으로 돌릴 수 있다는 것이 이 프로젝트의 유일한 개발 루프다
(→ `deterministic-probe`).

## 1. 게스트 주소공간

게스트가 만질 수 있는 것 전부를 **하나의 연속된 호스트 할당**에 놓는다.
게스트 주소 → 호스트 오프셋이 뺄셈 하나가 되고, 이게 중요하다: 해석기는
**모든 로드·모든 스토어·모든 명령어 인출마다** 이 변환을 한다.

```text
  0x0100_0000  image      .so 의 PT_LOAD 세그먼트 + .bss
  0x0180_0000  hle        import 심볼 하나당 한 워드 (트램폴린)
  0x0181_0000  jni        합성 JNIEnv, 함수 테이블, 가짜 핸들
  0x0190_0000  heap       게스트 malloc 아레나
  0x0d90_0000  stack      위에서 아래로
```

규칙 셋:

1. **게스트 주소 0은 창 밖에 둔다.** 널 역참조가 조용히 ELF 헤더를 읽는 대신
   폴트가 되게.
2. **폴트는 대역 밖으로 보고한다.** 해석기의 `Memory` 트레이트는 보통 실패할 수
   없으므로, 범위를 벗어난 접근은 `Option<Fault>` 에 기록하고 실행 루프가 매
   스텝 뒤에 확인한다. `take()` 가 아니라 `is_some()` 으로 먼저 검사할 것 —
   `take` 는 매 명령마다 `Option` 을 되쓴다.
3. **메모리 접근 코드의 모양이 성능을 좌우한다.** 바이트 인덱싱 4번이 아니라
   `get(o..o+N)` + `try_into()` 로 쓴다. 전자는 각각 경계 검사되는 4번의 로드고
   백엔드가 항상 합쳐 주지는 않는다. 실측으로 해석기 처리량의 **약 1/3**이
   여기서 갈렸다.

`RETURN_MAGIC` — 호스트가 게스트 함수를 부를 때 LR 로 밀어 넣는 값. **매핑 창
바깥**의 주소로 잡아서, 그리로 잘못 분기한 코드가 진짜 코드로 오인되지 않게
한다. PC 가 이 값이면 실행 루프가 멈춘다.

## 2. ELF32 로더 — 실제로 필요한 것만

`seam_census` 가 relocation 세 종류만 보고했다면 로더는 200줄이면 된다.

* `PT_LOAD` 를 베이스에 복사한다. `.bss` 는 `memsz - filesz` 이고 게스트 RAM 이
  0으로 시작하므로 **파일에 있는 부분만** 복사하면 된다.
* `PT_ARM_EXIDX` 는 주소·길이를 기억해 둔다 — `__gnu_Unwind_Find_exidx` 가
  묻는다.
* 심볼은 **`.symtab` 이 있으면 그것부터**, 없으면 `.dynsym`. 이름→주소 맵 하나면
  된다. import 목록은 **`.dynsym` 인덱스 순서를 지켜야 한다** — relocation 이
  심볼을 그 인덱스로 지목한다.
* relocation:
  * `R_ARM_RELATIVE` — 대상에 이미 들어 있는 값이 addend 다. `base + addend`.
  * `R_ARM_GLOB_DAT` / `JUMP_SLOT` / `ABS32` — 이름을 resolve 한다. `DT_SYMBOLIC`
    바이너리는 자기 안에서 정의된 심볼도 이 relocation 으로 나오므로, resolve 가
    실패하면 **자기 심볼 테이블에서 한 번 더 찾아본다**.
  * resolve 실패는 슬롯을 0으로 두고 **이름을 모아서 에러로 올린다**. 조용히
    넘어가면 나중에 주소 0으로 점프하는 것으로 나타난다.
* 모르는 relocation 타입은 **에러다.** 무시하면 안 된다.

## 3. 경계면 구현 — 트램폴린 한 워드씩

import 심볼마다 HLE 창에 **한 워드**를 배정하고, 그 심볼의 `JUMP_SLOT`
relocation 이 그리로 향하게 한다. 게스트가 PLT 를 통해 `bl` 하면 PC 가 그
주소가 되고, 실행 루프는 **명령어를 인출하기 전에** 그 PC 를 알아본다:

```rust
loop {
    let pc = cpu.pc();
    if pc == RETURN_MAGIC { return Ok(()); }
    if let Some(id) = tramp_index(pc) { self.dispatch(id)?; continue; }
    ...
}
```

이 구조의 값어치:

* 게스트 코드를 **한 바이트도 고치지 않는다**.
* 미구현 import 는 **이름과 함께 죽는다**. 0을 돌려주고 넘어가는 것보다 백 배
  낫다 — 조용한 0은 30만 명령 뒤에 검은 화면으로 나타난다.
* JNIEnv 도 같은 방식이다. 함수 테이블의 **모든 슬롯**에 트램폴린을 채우고,
  구현 안 한 슬롯은 슬롯 이름과 함께 abort 하게 한다.

### 데이터 import 는 함수가 아니다

`__stack_chk_guard` 같은 것은 트램폴린이 아니라 **저장 공간**으로 resolve 해야
한다. 함수 트램폴린을 주면 게스트가 그 주소의 워드를 읽어 카나리로 쓴다.

### 게스트 함수를 호스트가 부를 수 있어야 한다

`qsort` 의 비교자, 타이머 콜백, 그리고 나중에 **호스트 포트의 자기 검증**까지
전부 이게 필요하다. 재진입 가능해야 한다:

```rust
pub fn call(&mut self, func: u32, args: &[u32]) -> Result<u32, String> {
    let saved = self.cpu;          // 통째로 저장
    // r0..r3 에 4개, 나머지는 스택에 8바이트 정렬로
    // LR = RETURN_MAGIC, PC = func
    let r = self.run();
    let ret = self.cpu.r0();
    self.cpu = saved;              // 통째로 복원
    r?; Ok(ret)
}
```

## 4. 플랫폼 생명주기를 그대로 재현한다

원본의 Java(또는 네이티브 액티비티) 쪽이 **어떤 순서로** 초기화했는지가 정답이다.
`classes.dex` 를 뜯어서라도 그 순서를 그대로 따른다. 순서가 틀리면 대개
"아무 일도 안 일어난다".

전형적인 형태:

```java
nativeInitKernel();
nativeInitDisplay(sw, sh, r,g,b,a, logicalW, logicalH, marginX, marginY, ...);
nativeInitFileStorage(internalPath, externalPath);
nativeInitVFS(fd, offset, length);      // 에셋 아카이브를 fd+범위로 넘긴다
nativeInitAudioConfiguration(...);
nativeStartApp();
// 이후 매 프레임: nativeLoop()
```

### 논리 화면 크기는 상수가 아니라 **인자**다

`nativeInitDisplay` 에 넘기는 폭/높이는 바이너리에 박힌 값이 아니라 플랫폼이
정해 준 값인 경우가 많다. 그렇다면 게스트는 그 값으로 프레임버퍼를 잡고,
디스플레이 정보 API 로 되돌려주고, **UI 를 그 폭 기준으로 스스로 중앙정렬한다**.

즉 **다른 값을 줘 보는 것은 공짜 실험이고, 대개 통한다.** 현대 폰의 화면비에서
좌우 검은 띠를 없애는 가장 정직한 방법이 이것이다:

```
논리폭 = floor(논리높이 * 물리폭 / 물리높이)     ← 내림
```

내림해서 남는 1~2px 은 가로 스트레치로 덮는다. 프로브에 `--wipi WxH` 같은
플래그를 두어 착수 초기에 확인해 둘 것. (게임이 그 폭을 안 따라오면 그때
포기하면 된다 — 확인 비용이 몇 분이다.)

## 5. 첫 프레임이 안 나올 때 — 순서대로 의심할 것

### (a) 가상 시계를 안 돌렸다 — **가장 흔하다**

게스트가 플랫폼 타이머로 idle 하는 구조면, 시간이 안 가면 **아무것도 안 그린다**.
헤드리스 러너는 매 프레임 `advance_time(ms)` 를 반드시 불러야 한다. 검은 화면의
1순위 원인이고, 증상이 "멈춤"이 아니라 "정상적으로 아무 일도 안 함"이라 알아채기
어렵다.

### (b) 해석기가 어떤 인코딩을 조용히 놓치고 있다

크래시 없이 화면 일부만 안 그려지면 이걸 의심한다. **간접 호출(`blx <reg>`)이
no-op 으로 실행되는 부류의 버그는 겉으로 완전히 정상으로 보인다.**
→ `cpu-differential-test` 스킬. 이건 추측하지 말고 기계적으로 확인하는 문제다.

### (c) 에셋 조회의 이름↔id 왕복이 안 맞는다

아카이브에서 리소스를 못 찾으면 폰트부터 실패해서 널 역참조로 죽는다.
→ `legacy-assets-and-encoding` 스킬.

### (d) 초기화 인자 하나가 0이다

`nativeInitDisplay` 의 마진/배율, VFS 의 offset/length, 저장소 경로 —
하나라도 0이나 빈 문자열이면 게스트가 조용히 포기하는 경로가 있다.
`CS_TRACE` 같은 **HLE 호출 전수 추적 스위치**를 처음부터 만들어 둘 것.

## 6. 폴트 보고를 아깝게 여기지 마라

브링업 기간에 가장 값어치 있는 코드는 **에러 메시지**다.

* 폴트가 나면 게스트 PC 를 `name+offset` 으로 찍는다(심볼이 살아 있다면).
* 레지스터 전체를 찍는다.
* 스택을 훑어 **이미지 안의 `bl` 바로 뒤를 가리키는 워드**를 모아 유사
  백트레이스를 만든다. 프레임 포인터가 없으므로 과다 보고되지만, 이름이 붙어
  있으면 몇 초 만에 경로가 보인다.

```
guest read of 4 bytes at 0x00000000 from MX_fntSetProperty+0x3c — outside the mapped window
r0=0x00000000 r1=0x0190a2c0 ...
sp=0x0dafff20 lr=0x0102a4b8 (FntInvalidate+0x88)
```

이 한 줄이 하루를 아낀다.

## 7. 다음 단계

첫 프레임이 나오면 → **`deterministic-probe`** (개발 루프 고정) →
**`cpu-differential-test`** (해석기 신뢰) → **`frame-optimization`** (프레임 예산).
