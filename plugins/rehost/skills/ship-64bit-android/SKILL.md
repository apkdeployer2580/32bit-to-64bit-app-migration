---
name: ship-64bit-android
description: Package and ship a rehosted guest as a 64-bit-only Android app without breaking players' saves — release signing and keystore discipline, update compatibility, process lifecycle, surface/rotation handling, touch pointer ids, and device performance logging. Use when building a release APK, setting up signing, an update refuses to install, saves disappear, the app dies after exit, or the pad/joystick sticks. 릴리스 빌드, 서명, 세이브 보존, 업데이트 실패, 종료 후 실행 불가.
---

# 64비트 앱으로 출하하기 — 세이브를 지키면서

## 0. 서명 키를 잃으면 끝이다

배포한 APK 를 **업데이트**하려면 매번 같은 키로 서명해야 한다. 다른 키로 서명한
APK 는 `INSTALL_FAILED_UPDATE_INCOMPATIBLE` 로 거부되고, 뚫는 길은 uninstall 뿐인데
그것은 앱의 내부 저장소를 통째로 지운다 — **플레이어 세이브가 사라진다.**

규칙:

1. **키스토어는 저장소 바깥.** 자격증명은 `~/.gradle/gradle.properties` 같은 곳에.
   빌드 스크립트는 환경변수 → gradle 속성 순서로 읽게 한다.
2. **인증서 지문이 앱의 신원이다.** 배포하는 모든 빌드에서 같아야 한다. 빌드
   끝에 자동으로 찍고, 다르면 그 APK 를 뿌리지 마라.
   ```bash
   apksigner verify --print-certs app-release.apk
   ```
3. **디버그 서명본을 배포하지 마라.** `~/.android/debug.keystore` 는 머신마다
   다르고 언제든 재생성된다. 그것으로 서명한 것을 뿌리면 다음 업데이트가
   설치되지 않고 사용자에겐 uninstall 외에 방법이 없다.
4. **디버그 서명본과 릴리스 서명본은 서로 덮어쓸 수 없다.** 기기에 한쪽이 깔려
   있으면 다른 쪽을 넣기 전에 지워야 하고, 그 시점에 세이브가 사라진다. 개발
   기기에서도 이것을 의식하고 다녀라.
5. 여러 앱을 같은 키스토어 **파일**에 다른 **alias** 로 두는 것은 안전하다.
   alias 추가는 기존 항목을 건드리지 않는다(추가 후 기존 지문이 그대로인지
   확인하라).
6. `minSdk >= 24` 면 **v2 서명만** 붙는다(v1 은 API 24 미만 전용). 키 교체(v3
   rotation)는 나중에 `--lineage` 로 추가할 수 있다.

업데이트가 제대로 됐는지는 **설치 시각**으로 본다:

```bash
adb shell "dumpsys package <pkg> | grep -E 'versionCode|InstallTime|UpdateTime'"
```

`firstInstallTime` 이 그대로면 세이브가 살아 있는 업데이트다.

## 1. ABI 는 하나만

산출물이 진짜로 64비트 단독인지 확인하라:

```bash
aapt2 dump badging app-release.apk | grep native-code
#  -> native-code: 'arm64-v8a'
```

원본 32비트 `.so` 는 **에셋으로** 들어간다(`lib/` 이 아니라 `assets/`). 그래야
패키지 매니저가 그것을 ABI 로 보지 않는다.

## 2. `exitApp` 이 앱을 영구히 죽일 수 있다

게스트가 종료를 요청할 때 네이티브 진입 함수가 그냥 return 하면, **글루 코드가
프로세스를 죽이지 않는 경우가 있다.** 그러면:

1. 다음 실행이 살아남은 프로세스를 재사용하고,
2. 글루가 진입 함수를 **두 번째로** 호출하고,
3. 이벤트 루프 생성이 "이미 만들어졌다"로 거부하며 패닉하고,
4. 액티비티가 그리기도 전에 죽는데 **프로세스는 여전히 안 죽으므로** 매 실행
   반복이다.

**앱이 영구히 실행 불가가 된다.** 루프가 끝나면 명시적으로 프로세스를 종료하라
(`std::process::exit(0)`). 그리고 그렇게 해도 안전하도록 **세이브 쓰기를 버퍼링
없이** 하라.

> 재현 테스트는 반드시 **런처로 재실행**해서 한다. `am force-stop` 을 먼저 걸면
> 증상이 가려진다. 종료 메뉴가 있으면 그쪽이 재현이 빠르다.

## 3. 화면 — 회전과 크기 변경

* 프레임버퍼는 부팅 때 한 번 정해진다. 서피스가 나중에 달라지면(회전 중,
  분할화면, 폴드) **그대로 memcpy 하면 화면이 대각선으로 밀린다.** present 경로가
  크기 불일치를 감지해 **스트라이드를 지키는 최근접 스케일**로 폴백해야 한다.
* 논리 해상도를 화면비에서 만들면 좌우 검은 띠가 없어진다(→ `rehost-bringup` §4).
  정수로 안 떨어지는 기기는 **내림**하고 남는 1~2px 을 가로 스트레치로 덮는다.
* 180° 회전은 매니페스트의 `sensorLandscape` 가 이미 허용한다.

## 4. 터치 포인터 id — 조이스틱이 굳는 원인

게스트가 "지금 조이스틱을 잡고 있는 손가락"을 **부호 있는 바이트**에 저장하고,
해제할 때 `(s8)stored == released_id` 를 요구하는 구조가 흔하다. 그러면:

* 플랫폼의 raw 포인터 id 가 그 바이트로 표현 안 되면 **영원히 해제되지 않는다.**
* **릴리스가 안 온 프레스**가 하나라도 있으면 그 세션 내내 패드가 마지막 자리에
  붙는다.

호스트가 할 일 두 가지:

1. 플랫폼의 raw id 대신 **0..N 슬롯 번호**를 게스트에 넘긴다. 왕복이 구성상
   정확해진다.
2. **`resumed` 에서 아직 눌려 있던 손가락에 대해 릴리스를 대신 쏜다.** 액티비티가
   백그라운드로 갈 때 플랫폼은 그 릴리스를 주지 않는다.

## 5. 기기 성능 로그

```
perf: guest 12.4ms  copy 0.8ms  present 1.9ms   46.1 M inst/s
      host: 35162 screen, 1204 rot, 88 fnt, 2 declined
```

* **guest / copy / present 를 쪼개서** 찍어라. "해석기가 느리다"와 "presenting 이
  느리다"는 완전히 다른 문제다.
* `M inst/s` 를 실측해서 상수로 박아 두면 프로브의 명령 수를 사람이 느끼는 ms 로
  바꿀 수 있다 (→ `frame-optimization`).
* **호스트 패치 카운터를 기기 로그에도** 넣어라. 데스크톱에서는 서브되는데
  기기에서는 0인 경우가 실제로 있다(심볼 해석 실패, 빌드 시점 스위치 차이).

### 로거 필터를 확인하라

안드로이드 로거를 `warn` 기본으로 두면 **core 크레이트의 info 로그가 통째로
버려진다.** 디버그 스위치를 켰는데 기기에서 아무것도 안 찍히는 이유가 대개
이것이다. `<core_crate>=info` 를 명시적으로 통과시켜라.

## 6. 빌드 시점 스위치의 함정

기기에는 환경변수를 설정할 방법이 없으므로, 설정값은 **환경변수와 빌드 시점
둘 다** 읽게 만든다:

```rust
fn knob(name: &str, default: i32) -> i32 {
    std::env::var(name).ok().and_then(|v| v.parse().ok())
        .or_else(|| option_env!("...").and_then(|v| v.parse().ok()))
        .unwrap_or(default)
}
```

**함정**: `option_env!` 는 환경변수를 바꿔도 **재빌드되지 않는다.** 값을 바꾸려면
해당 크레이트를 `cargo clean -p <crate>` 해야 한다. 서버 주소를 바꿨는데 옛 주소로
붙는 현상의 원인이 이것이다. 빌드 스크립트에 적어 두어라.

## 7. 호스트 툴체인 특이사항 (Windows/GNU)

* 안드로이드 크로스컴파일이어도 **호스트** build-dependency 가 빌드되고, 그
  raw-dylib import 라이브러리 때문에 rustc 가 `dlltool.exe` 를 찾는다. rustup 의
  GNU dlltool 은 짝인 `as` 가 없어 못 쓴다 → 심을 만들어 **PATH 앞**에 둔다.
  `target.<host>.rustflags` 는 build-dep 에 닿지 않으므로 PATH 여야 한다.
* AGP 8.x 는 JDK 17+ 가 필요하다. PATH 의 첫 `java` 가 Java 8 이면
  `JAVA_HOME` 을 **언급하지 않는** 메시지로 죽는다. 빌드 스크립트가 JDK 를 직접
  고르게 하라.
* PowerShell 빌드 스크립트는 `$ErrorActionPreference = 'Continue'` 로 둬라.
  cargo/gradle 이 진행 상황을 stderr 로 쓰는데 `'Stop'` 이면 첫 줄에서 죽는다.
  같은 이유로 `cargo … 2>&1` 을 쓰지 마라.

## 8. 릴리스 체크리스트

```
[ ] -Release 로 구웠다 (디버그 서명본 아님)
[ ] apksigner verify 지문이 이전 릴리스와 같다
[ ] versionCode 가 올라갔다
[ ] native-code 가 arm64-v8a 하나뿐이다
[ ] 결정적 재생 A/B 로 회귀가 없다 (의도한 동작 변경만 다름)
[ ] 프레임 분포: 예산 초과 프레임 수가 이전 릴리스 이하다
[ ] 실기기에서 종료 → 런처로 재실행이 정상이다
[ ] 실기기에서 업데이트 설치 후 firstInstallTime 이 그대로다
[ ] 세이브 파일 해시가 업데이트 전후로 같다
```
