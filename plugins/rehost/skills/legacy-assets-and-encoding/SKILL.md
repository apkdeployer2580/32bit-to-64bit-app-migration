---
name: legacy-assets-and-encoding
description: Reverse a legacy game's asset archive and keep its legacy text encoding byte-exact through the host. Covers archive directory formats, name-to-id registries that must round-trip, hash lookups, and the class of bug where routing legacy-encoded strings through a UTF-8 string type silently rewrites them into plausible-looking garbage. Use when assets fail to load, fonts crash at boot, text renders as wrong-but-readable characters, or when implementing file/VFS HLE. 에셋 아카이브, VFS, 인코딩 깨짐, 글자 깨짐, EUC-KR/Shift-JIS.
---

# 에셋 아카이브와 레거시 인코딩

브링업에서 죽는 두 가지가 여기 있다. 둘 다 **조용히** 틀리는 부류다.

## 1. 게스트 문자열은 UTF-8 이 아니다 — 절대 `String` 을 거치지 마라

한국/일본/중국 레거시 게임의 문자열은 EUC-KR·CP949·Shift-JIS·GBK 다.
호스트가 이것을 `String::from_utf8_lossy` 로 읽으면 **0x80 이상 바이트가 전부
U+FFFD** 가 되고, 다시 UTF-8 로 쓰면 **완전히 다른 바이트**가 된다.

그리고 여기가 함정이다: **게임 폰트는 아무 2바이트나 어떤 글자로든 그려 준다.**
그래서 화면에는 크래시도 물음표도 아니고 **그럴듯한 글자**가 나온다. "썍몌예ㅃ
쏙옙" 같은 것이 보이면 이 버그다. 인코딩이 깨진 줄 모르고 폰트 렌더러를 며칠
파게 된다.

### 규칙

**게스트로 되돌아가는 모든 값은 바이트 단위로 다뤄라.**

| 대상 | 어떻게 |
|---|---|
| `strcpy/strncpy/strcat/strcmp/strncmp/strlen/strstr/strdup/vsnprintf` | 전부 **바이트**. `cstr_bytes()` → `Vec<u8>` |
| JNI 문자열 (`NewStringUTF` / `GetStringUTFChars`) | 문자열 객체를 `Vec<u8>` 로 보관. 왕복이 바이트를 보존해야 한다 |
| 에셋 이름 등록부 | 키가 `Vec<u8>` |
| 이름 해시 계산 | **원시 바이트**를 해시한다 |
| 호스트가 직접 소비하는 값 (파일 경로, 로그) | UTF-8 lossy 로 읽어도 된다. 대개 ASCII다 |

두 함수를 명확히 나눠 두고 이름으로 구분하라: `cstr_bytes()`(게스트로 돌아감) /
`cstr()`(호스트가 소비). 그리고 후자의 독스트링에 **"게스트로 되돌아가는 값에는
절대 쓰지 말 것"** 이라고 적어라.

### 문자열이 어디에 있는지 세어 두면 좋다

```bash
python - <<'EOF'
import re, pathlib
b = pathlib.Path('libgame.so').read_bytes()
# EUC-KR 완성형 한글: 0xB0-0xC8 / 0xA1-0xFE
hits = re.findall(rb'(?:[\xb0-\xc8][\xa1-\xfe]){2,}', b)
print(len(hits), "candidate strings")
for h in hits[:20]: print(h.decode('euc-kr', 'replace'))
EOF
```

`.rodata` 에 617개, 같은 식이다. 이 목록이 있으면 나중에 문구를 바꾸고 싶을 때
"호스트 문자열을 만들어 EUC-KR 로 재인코딩"하지 않고 **바이너리 안에 이미 있는
다른 문자열로 갈아끼우는** 훨씬 안전한 수를 찾을 수 있다.

## 2. 아카이브 포맷 — 디렉터리 스캔 함수를 디스어셈하라

포맷은 추측하지 말고 **조회 함수 하나를 읽어서** 확정한다. 전형적으로:

```
[u32 nameHash][u32 size][size 바이트] [u32 nameHash][u32 size][...] ...
```

`xref.py` 로 `fread`/`fseek` 호출자를 찾고, 그중 이름을 인자로 받는 것을
`disasm.py` 로 읽으면 몇 분이면 나온다.

### 이름 → id 는 해시가 아닐 수 있다 — 이게 첫 부팅을 죽인다

실제 사례: 플랫폼 쪽 매니저가 해시테이블 두 개로 **"처음 본 순서대로 1부터"**
번호를 붙이고, 로드 함수가 그 번호로 이름을 되찾아 **그때 해시를 계산**한다.
즉 id 는 해시와 아무 관계가 없고, **왕복(name → id → name → hash)을 그대로
재현하지 않으면** 리소스를 못 찾는다.

증상: 폰트 리소스 로드 실패 → 폰트 속성 설정에서 널 참조로 죽음.
브링업 초기에 나는 크래시의 단골이다.

호스트에 그 등록부를 그대로 두어라:

```rust
asset_ids:   HashMap<Vec<u8>, i32>,   // 처음 본 순서대로 1부터
asset_names: HashMap<i32, Vec<u8>>,
```

### 조회를 매번 재스캔하면 그 자체가 병목이다

원본이 조회할 때마다 아카이브 전체를 4바이트씩 훑는 구현이면(흔하다) 부팅에서
메뉴까지 가는 동안 `fread` 가 **87만 회** 나온다. 호스트에서 **인덱스를 한 번
빌드**해 두고 그 함수를 서브하라 (→ `frame-optimization`).

인덱스를 만들 때 게스트의 러닝 카운터를 정확히 흉내 내라 — 게스트가 돌려주는
오프셋이 **헤더를 지난 뒤의 값**인지 아닌지가 다르면 전부 어긋난다.

```rust
let mut cursor = 0u32;
while off + 8 <= end {
    let (hash, size) = (u32le(off), u32le(off + 4));
    if size == 0 || off + 8 + size > end { break; }
    cursor += 8;                       // 게스트가 저장하는 오프셋은 여기 값
    map.entry(hash).or_insert((cursor, size));
    cursor += size;
    off += 8 + size as usize;
}
```

`or_insert` — **먼저 나온 레코드가 이긴다.** 중복 해시가 있을 때 게스트의 선형
스캔과 같은 답을 내려면 그래야 한다.

### 확장자를 믿지 마라

`data.vfs.mp3` 가 19 MB 짜리 게임 아카이브인 식이다(그 안에 진짜 Ogg 도 들어
있다). 플랫폼이 에셋을 `AssetFileDescriptor` 로 열어 **fd + offset + length** 를
네이티브에 넘기는 구조였다면, 호스트도 같은 삼중조를 넘겨야 한다.

## 3. 아카이브 안에 뭐가 들었는지 먼저 세어라

```bash
python - <<'EOF'
import pathlib, struct, collections
b = pathlib.Path('payload/assets.bin').read_bytes()
off, n, kinds = 0, 0, collections.Counter()
while off + 8 <= len(b):
    h, sz = struct.unpack_from('<II', b, off)
    if sz == 0 or off + 8 + sz > len(b): break
    rec = b[off+8:off+8+16]
    kinds[rec[:4]] += 1
    n += 1; off += 8 + sz
print(n, "records"); 
for k, c in kinds.most_common(12): print(c, k)
EOF
```

앞 4바이트로 종류를 세면 `OggS`, `RIFF`, `PNG`, 자체 포맷이 바로 갈린다.
"소리가 안 난다"의 답이 여기서 나온다 — 12.6 MB 가 Ogg 였다는 식.

오디오라면 게스트가 PCM 을 보지 않고 **아카이브 안의 바이트 범위**를
`(clipId, offset, length)` 로 넘기는 구조일 가능성이 높다(원본에서 그것이 그대로
`MediaPlayer.setDataSource(fd, offset, length)` 였기 때문). 디코드는 호스트가
한다. 그렇다면:

* 같은 클립을 **재디코드하지 마라.** 소스 맵을 기록만 하고 조회하지 않는 버그가
  흔하다 — 맵을 옮길 때마다 같은 BGM 을 처음부터 다시 디코딩한다.
* **디코드를 프레임 스레드에서 하지 마라** (→ `frame-optimization` §7).

## 4. 세이브 파일

* **엔트로피를 재 보라.** 0.1 bits/byte 면 평문이고, 7.9 면 암호화/압축이다.
  "이건 암호화된 세이브겠지"라고 가정하고 며칠 파는 일을 막아 준다.
* 암호화라면 → `frame-optimization` 의 자기 검증 절(테이블을 하드코딩하지 말고
  **게스트 메모리에서 심볼 이름으로 읽어라**).
* 세이브 경로는 호스트가 정한다. **`BufWriter` 없이 바로 쓰는 것**을 고려하라 —
  프로세스가 어떻게 끝나든 데이터가 남는다.
