---
name: deterministic-probe
description: Build and use the headless deterministic replay harness for a rehosted guest — virtual clock, scripted keys and taps, PNG frame dumps, guest PC profiling, memory peeks, per-frame cost. This is the development loop; every performance claim and every A/B proof depends on it being reproducible. Use when setting up the probe runner, replaying a bug, dumping frames, or when two runs disagree. 헤드리스 러너, 결정적 재생, 프레임 덤프, 프로파일.
---

# 결정적 헤드리스 프로브 — 이 프로젝트의 유일한 개발 루프

기기 없이, 창 없이, **같은 입력이면 같은 바이트**가 나오게 게스트를 돌리는 러너.
안드로이드 앱과 **같은 core** 를 쓰므로, 여기서 재현되는 문제는 기기에서도 같은
문제다.

이것이 없으면 "빨라졌다"도 "안 깨졌다"도 증명할 수 없다. 그래서 최적화보다
먼저 만든다.

## 1. 최소 인터페이스

```bash
probe --so payload/libgame.so --vfs payload/assets.bin \
      --frames 2600 --ms 33 \
      --keys 210:LEFT,230:OK --taps 60:1301x605 \
      --dump 1,200,400,600 --out out/ --storage out/storage \
      --profile --peek 0x1172f59:64
```

| 플래그 | 왜 있어야 하는가 |
|---|---|
| `--ms` | **가상 시계 증분. 사실상 필수다.** 게스트가 타이머로 idle 하면 시간을 안 돌릴 때 화면이 영원히 검다 |
| `--frames` | 재생 길이 |
| `--keys F:KEY` | 프레임 F 에 키 press+release |
| `--taps F:XxY` | 프레임 F 에 물리 서피스 픽셀 좌표로 터치. `adb shell input tap` 과 **같은 좌표계**로 두면 기기 세션을 그대로 옮길 수 있다 |
| `--dump F,F,F` | 그 프레임을 PNG 로 |
| `--out` / `--storage` | 출력·세이브 디렉터리 |
| `--profile` | 게스트 PC 샘플링 → 함수 이름별 히스토그램 |
| `--peek A:N` | 끝에 게스트 메모리 범위 요약(비-0 비율 + 앞 24바이트) |

### 터치는 press 와 release 를 **분리해서** 넣어라

같은 틱에 눌렀다 떼면 길이 0인 터치다. 대부분의 UI 는 press 에서 준비하고
**뒤따르는 release 에서 실행**한다. 프로브는 press 후 2프레임 뒤에 release 를
쏘는 식으로 고정해 두는 것이 좋다.

## 2. 결정성을 깨는 세 가지 — 전부 겪게 된다

**1) 세이브 디렉터리가 실행 간에 이월된다.**
가장 흔하고 가장 오래 헤맨다. 두 번째 실행이 첫 실행의 세이브를 읽으면 재생이
갈라진다. **매 실행 새 `--storage` 디렉터리를 주거나 지워라.** A/B 비교 전에
반드시 확인할 것.

**2) 벽시계를 쓴다.**
`virtual_clock` 이 꺼져 있으면 프레임마다 다른 시간이 흐르고, 타이머 기반 상태
전이가 실행마다 달라진다. 프로브는 항상 가상 시계다.

**3) 비동기 작업이 프레임 경계를 넘는다.**
오디오 디코드를 워커 스레드로 뺐다면(기기에서는 그래야 한다) 프로브에서는
**동기로 되돌려라.** 그래야 오디오 출력까지 재현 가능하다. 설정 플래그 하나로
갈라 두는 것이 맞다: 기기 `async = true`, 프로브 `async = false`.

## 3. 매 실행 끝에 반드시 찍어야 하는 것

```
frame cost (guest inst -> ms at 46 M inst/s): mean 1234567 (26.8), p50 ..., p90 ..., p99 ..., max ...
  12/2600 frames over the 33 ms budget (0.5%)
  worst frames 983:48ms 1:37ms 2100:35ms ...
done: 2600 frames, 7.2e7 guest instructions, 94.1 M inst/s, 120.4 fps
host patches: N blits, N rot, N screen, ... , N declined to the guest
--- guest hot functions (PC samples) ---
   8.0%    12043  desRound
   5.2%     7821  MONSTER_dataLoad
```

각각의 이유:

* **프레임 비용 분포** — 평균은 "보스전 뒤에 렉이 걸린다"를 절대 못 보여 준다.
  p99 와 최악, 그리고 예산 초과 프레임 수가 진짜 지표다 (→ `frame-optimization`).
* **가장 비싼 프레임 번호** — 다음 명령이 곧바로 `--dump <그 번호>` 다. 번호를 안
  찍으면 그 프레임을 찾는 데 몇 분이 든다.
* **`M inst/s`** — 데스크톱 처리량. 기기 값과 함께 알아야 ms 환산이 의미를 가진다.
* **호스트 패치 카운터와 거절 수** — **0이면 그 패치는 설치조차 안 된 것이다.**
  이 한 줄이 "이식했는데 왜 안 빨라지지"를 즉시 해결한다.

프레임 비용 리스트를 파일로 뽑아 두면 전/후 비교가 한 줄이다:

```bash
python ${CLAUDE_PLUGIN_ROOT}/scripts/frame_stats.py after.txt --before before.txt \
    --inst-per-sec 46e6 --budget-ms 33
```

## 4. 게스트 PC 프로파일러 — 두 가지 주의

**(a) 샘플 주기는 카운트다운으로.**
`steps % PERIOD` 는 매 게스트 명령마다 64비트 나머지 연산이다. 실측으로 해석기
처리량의 **약 40%**를 먹었다(46 → 27 M inst/s). 감소 카운터로 쓸 것.

**(b) 기본으로 켜 두지 마라.** 기기 빌드에서 프로파일러가 항상 켜져 있으면 위
비용을 늘 낸다. 플래그로 켠다.

샘플은 PC 히스토그램이고, 리포트할 때 **가장 가까운 심볼 이름으로 접는다**.
`name+offset` 의 `+offset` 을 떼고 함수 단위로 합치면 바로 "무엇을 이식할지"의
목록이 된다.

## 5. `--peek` 와 함수 감시 — 눈이 아니라 데이터로 보기

* `--peek 0x1990e80:27789` — 그 범위의 비-0 비율을 찍는다. "에셋이 아무것도
  디코드 안 됐다"와 "그리기가 떨어뜨렸다"를 구분하는 가장 빠른 방법이다.
  실행이 결정적이므로 한 번 알아낸 주소는 다음 실행에서도 유효하다.
* **`WATCH=Sym1,Sym2` 환경변수로 게스트 함수 인자 추적.** 그 심볼에 후크를 걸되
  아무것도 안 하고 인자와 호출자만 찍고 통과시킨다. 2.7 KB 짜리 게스트 함수가
  *무엇을 그리라고 지시받는지* 볼 수 있는 사실상 유일한 방법이다. 재빌드 없이
  아무 함수에나 걸 수 있게 만들어 둘 것.

## 6. 기기 세션을 프로브로 옮기기

버그가 기기에서만 보인다면, 그 세션을 프로브 인자로 옮기는 것이 첫 수다.

1. `adb shell input tap X Y` 좌표를 그대로 `--taps` 에 넣는다(같은 좌표계로
   만들어 뒀다면 변환이 없다).
2. 기기 해상도를 `--w/--h` 로, 논리 해상도를 `--wipi` 로 맞춘다.
3. 메뉴 타일은 **한 번 탭해서 커서 이동, 한 번 더 탭해서 실행**인 경우가 흔하다.
   확인창의 「예」 위치는 버튼이 하나일 때(중앙)와 둘일 때(왼쪽)가 다르다 —
   중앙을 누르면 아무 일도 안 일어난다.

## 7. A/B 비교

```bash
NO_PATCH=1 probe ... --out out_ref   --storage out_ref/st
           probe ... --out out_test  --storage out_test/st
python ${CLAUDE_PLUGIN_ROOT}/scripts/ab_compare.py out_ref out_test
```

`ab_compare.py` 는 **픽셀 단위**로 비교하고, 다르면 첫 번째 다른 픽셀의 좌표와
값을 찍는다. 이것이 모든 호스트 이식의 합격 기준이다 (→ `frame-optimization`).

두 실행이 다를 때 코드를 의심하기 전에:
1. storage 를 새로 줬는가
2. 가상 시계였는가
3. **화면을 의도적으로 바꾸는 스위치**(버튼 추가, 가격 표시 변경 등)를 껐는가
