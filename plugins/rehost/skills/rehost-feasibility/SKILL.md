---
name: rehost-feasibility
description: Decide whether a 32-bit ARM game/app can be rehosted inside a 64-bit process, and produce the evidence for that decision. Measures the seam (imports, JNI entry points, callbacks), the architecture tags, symbol survival and loader hazards, then writes an ANALYSIS document. Use at the very start of a port — when asked to "convert a 32-bit APK to 64-bit", "make this old game run on a modern phone", "can this be ported", 32비트 게임 64비트 변환/이식 가능한지, 리메이크 가능성 판단.
---

# 32→64 재호스팅 — 가능한가, 그리고 그 근거

## 0. 먼저 말해야 하는 사실

**32비트 기계어를 64비트 기계어로 "변환"하는 도구는 없다.** 앱 본체가
ARM32 `.so`(또는 `.dex`가 아닌 네이티브 바이너리)이고 소스가 없다면, 재컴파일은
선택지가 아니다. 정적 리컴파일은 연구 수준이고 수십만 바이트 규모에서 신뢰할 수
없다.

**대신 가능한 것**: 프로세스 전체가 64비트인 앱 안에서 원본 32비트 코드를
**해석 실행**하고, 그 코드가 바깥세상과 만나는 **좁은 경계면(seam)만** 네이티브로
구현하는 것. 게임 로직은 출시된 바이너리 그대로, 그 주변만 64비트다.

사용자가 "64비트로 변환해줘"라고 말했을 때, 이 구분을 **먼저 한 문장으로**
말하고 넘어가라. 변환이 아니라 재호스팅이라는 것을 합의하지 않으면 이후 모든
결정이 어긋난다.

## 1. 판단은 오직 하나로 갈린다 — 경계면의 크기

바이너리가 **자기 안에서** 부르는 것은 전부 공짜다. 재구현하지 않는다.
비용을 정하는 것은 **바이너리를 떠나는 호출의 가짓수**뿐이다.

```bash
python ${CLAUDE_PLUGIN_ROOT}/scripts/seam_census.py <libgame.so>
python ${CLAUDE_PLUGIN_ROOT}/scripts/seam_census.py <game.apk> --extract payload/
```

이 스크립트가 내는 항목과, 각 항목이 **무엇을 결정하는지**:

| 항목 | 의미 |
|---|---|
| `imports` (undefined dynsym) | 손으로 구현해야 할 libc/libm/GL 함수의 개수. 이것이 곧 HLE 계층의 크기다 |
| `JNI entry points` | 플랫폼(Java)이 바이너리를 부르던 지점. 앱 셸이 재현해야 할 생명주기 |
| `.symtab` 생존 여부 | **가장 큰 비용 배수**. 이름이 살아 있으면 프로파일·폴트·후킹이 전부 원본 이름으로 말한다 |
| `Tag_CPU_arch` / Thumb / VFP / NEON | 해석기가 구현해야 할 명령어 집합. 여기서 틀리면 **크래시가 아니라 조용히 틀린 값**이 나온다 |
| relocation 히스토그램 | 로더의 난이도. `RELATIVE`/`GLOB_DAT`/`JUMP_SLOT` 셋뿐이면 교과서다 |
| `PT_TLS` / `.init_array` | 있으면 로더가 모델링해야 할 것이 늘어난다 |
| `.text` 크기 | 해석 실행이 감당할 수 있는 규모인가 |

### 대략의 눈금

* **imports ≤ 120** — HLE 계층은 며칠짜리다. 진행하라.
* **imports 120~400** — 유한하지만 실재하는 작업. 게으르게 구현하고, 미구현
  호출은 **이름과 함께 크게 죽어라**(조용히 0을 돌려주면 안 된다).
* **imports > 400** — 대부분이 한 라이브러리(예: 통째로 shim 가능한 SDK)인지
  먼저 확인하라. 아니면 재호스팅이 아니라 다른 접근을 검토해야 한다.

### 그래도 세어야 하는 것 하나 — Java 쪽 콜백

JNI 진입점은 "Java → 네이티브"다. 반대 방향, 즉 **네이티브가 부르는 Java 정적
메서드**는 ELF 에 안 나온다. `.rodata` 의 메서드 이름/시그니처 문자열과
`GetStaticMethodID` 호출을 세면 나온다:

```bash
python ${CLAUDE_PLUGIN_ROOT}/scripts/xref.py <libgame.so> GetStaticMethodID CallStaticIntMethod
strings -a <libgame.so> | grep -E '^\([A-Za-z\[;/]*\)[A-Za-z\[;/]+$'   # JNI 시그니처
```

경험적으로 이 숫자가 imports 보다 크면 앱 셸이 진짜 작업이고, 작으면 HLE 가
진짜 작업이다.

## 2. 그림도 같이 봐야 한다 — 게스트가 화면을 어떻게 만드는가

같은 경계면 크기라도 렌더링 방식이 비용을 완전히 바꾼다. 다음 순서로 확인한다:

1. GL/EGL import 가 **몇 개**인가. 19개 안팎이면 GLES1 이고, 십중팔구
   **텍스처 하나 + 사각형 하나**다.
2. `xref.py` 로 `glDrawArrays`·`glTexSubImage2D` 호출 지점을 찾아 호출자를
   본다. 호출자가 한두 개면 게스트는 **자기가 프레임버퍼에 직접 픽셀을 쓰고**
   GL 은 그것을 화면에 올리는 데만 쓴다.
3. 그렇다면 GL 구현은 필요 없다. **그 호출들을 지켜보다가 사각형 하나를 CPU 로
   확대 복사하는 상태 기계**면 충분하고, 그 덕에 헤드리스 러너와 실제 앱이
   완전히 같은 렌더 경로를 쓴다(이것이 나중에 결정적 재생을 가능하게 한다).

반대로 스프라이트마다 GL 호출이 나간다면 GL 계층을 진짜로 구현해야 하고,
헤드리스 검증도 훨씬 어려워진다. **이 사실은 착수 전에 알아야 한다.**

## 3. 산출물 — `ANALYSIS.md`

판단은 문서로 남긴다. 다음 세션(또는 다음 사람)이 같은 질문을 다시 하지 않게
하는 것이 목적이므로, **결론 먼저** 쓴다.

```markdown
# <게임> — 원본 분석과 64비트 이식 근거

## 1. 결론부터
원본 APK 를 64비트로 "변환"하는 것은 불가능하다. 본체는 <경로> 안의
<N> KB ARM32 기계어이고 소스가 없다.
대신 가능한 것: <구조 한 문단>. 경계면이 좁다는 것이 근거다:

| 경계면 | 크기 |
|---|---|
| import 심볼 | N |
| JNI 진입점 | N |
| 게스트가 부르는 Java 정적 메서드 | 약 N |
| 그 외 전부 | .so 안에 있음 |

## 2. 원본 바이너리의 정체
(seam_census 출력의 identity/hazards 절을 사실만 옮긴다)

## 3. 실행 구조
(원본 Java/플랫폼 쪽이 어떤 순서로 초기화하는가 — dex 를 뜯어서라도)

## 4. 그래픽
## 5. 에셋/아카이브 포맷
## 6. 남은 것
```

**항상 참인 짧은 사실**은 `CLAUDE.md` 에, **근거와 서술**은 `ANALYSIS.md` 에
둔다. 이 분리를 처음부터 지켜야 나중에 컨텍스트가 감당된다.

## 4. 원본을 옆에 놓을 수 있는지 확인하라

가장 확실한 정답지는 **원본이 아직 도는 32비트 기기**다. 요즘 폰도 일부는
`abilist` 에 `armeabi-v7a` 가 있다:

```bash
adb shell getprop ro.product.cpu.abilist
adb install -r <original32bit.apk>
```

원본을 나란히 놓고 비교할 수 있으면 "이게 버그인가 원래 그런가"를 몇 초에
판정할 수 있다. 실제로 겹쳐 보이는 폰트, 이상한 배율, 못생긴 UI 정렬 대부분이
**원본도 똑같다**. 이 확인이 가능한지 여부를 착수 전에 기록해 둔다.

## 5. 하지 말아야 할 판단

* **"일단 디컴파일해서 다시 짜자"** — 887 KB 기계어에 함수 2700개면 재작성이
  아니라 재발명이고, 원본과 다른 게임이 된다.
* **"에뮬레이터를 통째로 가져다 쓰자"** — 범용 에뮬레이터는 이 바이너리가 쓰지
  않는 것의 99%를 구현하고, 정작 필요한 이 게임 전용 경계면은 없다. 필요한 것은
  **CPU 해석기 하나**이고 그것만 가져다 쓴다(포크해서 이 아키텍처에 맞춘다 →
  `cpu-differential-test` 스킬).
* **"프레임이 느리면 그때 가서 최적화하자"** — 맞다. 다만 **측정 가능한
  구조**(결정적 헤드리스 러너 + 게스트 PC 프로파일러)를 처음부터 넣어야 그때가
  왔을 때 며칠이 아니라 몇 시간이 된다 → `deterministic-probe`, `frame-optimization`.

## 6. 다음 단계

가능하다는 결론이 나오면 → **`rehost-bringup`** 스킬.
