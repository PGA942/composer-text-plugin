# composer-text-plugin — ハッシュタグ候補(スロット `composer.text`)

コンポーザー本文で `#` を打つと、**最近使ったタグ**(`storage.safe` の `recent`)+ **組み込み辞書** から
前方一致の候補(最大 8 件)を FOW に出し、タップで本文の `#トークン` を `#タグ ` に置き換えるサンプル。

## このリポジトリの形(`rasq` ワークスペース)

`rasq-sdk-sample` の `docs/cli.md` §2 のワークスペース。`rasq init` / `rasq new composer.text` が作る形に合わせてある。

```
rasq.toml                                # schema = "rasq.workspace/1"、[[plugins]] に name/slot/id
Cargo.toml                               # [workspace](rasq CLI が rasq.toml から作り直す)
kit/                                     # rasq-plugin-kit の写し(正典は SDK の rust/rasq-plugin-kit)
plugins/composer-text-plugin/
  manifest.json                          # plugin.manifest の答えそのもの
  src/lib.rs                             # manifest.json を include_str! で返し、composer.text を実装
dist/                                    # rasq build --raw の出力(.rasqpkg と生の .wasm)
```

- 契約の正典は [`CONTRACT.md`](CONTRACT.md) が指す `rasq-sdk-sample/docs/slots.md` §2.3(ホスト関数は §4)。
- マニフェストは `plugins/composer-text-plugin/manifest.json` の 1 か所だけ。`src/lib.rs` は `include_str!` で
  そのまま返すので、`rasq check` の往復検査(wasm の `plugin.manifest` == `manifest.json`)が常に一致する。
  名前 / 版 / 説明 / 権限を変えるときは `manifest.json` を直す。

## 契約(`docs/slots.md` §2.3)

| fn | args | 戻り |
|---|---|---|
| `plugin.manifest` | `{}` | `manifest.json`(`{id: "sample.composer-text", kind: "action", slots: ["composer.text"], permissions: ["fow.widget", "storage.safe"]}`) |
| `lifecycle` | `{kind, surface?}` | `{}` |
| `text.changed` | `{token, text, selectionStart, selectionEnd, trigger?}` | `{}`(副作用で `fow.open` / `fow.close`) |
| `pick.<i>` | `{nodeId, kind}`(FOW の `ListTile` の tap) | `{}`(副作用で `composer.text.replace` / `storage.set` / `fow.close` / `toast`) |
| `suggestClosed` | `{id, reason}`(ユーザーが ✕ で閉じた) | `{}`(状態を捨てるだけ。`fow.close` は呼ばない) |
| その他 | | `C01` |

### call / ホスト関数の流れ

```
ホスト → plugin.manifest
ホスト → text.changed {token, text, selectionStart, selectionEnd, trigger?}
            trigger が無ければ自分で判定: selectionStart から [^\s#@] を遡り、その直前が `#` なら
            `#<run>` がトリガー(範囲は UTF-16 code unit = Dart の String オフセット)
            トリガーあり → storage.get recent
                         → fow.open {id:"hashtag-suggest", spec:{title:"ハッシュタグ候補",
                                     initialHeightFactor:0.35, attachTop:false},
                                     onClosed:"suggestClosed", root:[ListTile × 候補]}
            トリガー無し → (開いていれば) fow.close
                                                   ← {}
FOW の ListTile → pick.<i>
            → composer.text.replace {token, start, end, text:"#<tag> "}
            → storage.set recent(先頭に追加、重複除去、最大 20)
            → fow.close → toast "#<tag> を挿入しました"       ← {}
ホスト → suggestClosed(ユーザーが ✕ で閉じた)/ lifecycle   ← {}(状態を捨てる)
```

- 同じ `id` で `fow.open` し直すので、入力が進むたびに中身が差し替わる。
- `trigger` をホストが渡す場合は `"#タグ"`(文字列)か `{text, start, end}` を受ける。`@` / `:` で始まるものは対象外。
- 候補が 0 件でも `#` トークンがある間は FOW を出す(「候補はありません」)。
- 候補の並びは 最近使ったタグ(新しい順)→ 組み込み辞書(打上会 / スペースライフ / 宇宙 / ロケット発射 /
  写真 / 料理 / 音楽 / ゲーム / 雑談 / RASQ)。重複は除き、前方一致は大文字小文字を区別しない。

## ビルドと確認

`rasq` CLI は `rasq-sdk-sample` から組む(`cd rust && cargo build --release -p rasq-cli`)。

```sh
rasq check                 # wasm32 ビルド → export 検査 → manifest 往復 → スキーマ / 登録簿 → cargo test
rasq test                  # cargo test だけ
rasq build --raw           # dist/sample.composer-text-0.1.0.rasqpkg と dist/composer_text_plugin.wasm
rasq inspect dist/sample.composer-text-0.1.0.rasqpkg
```

native テストはトリガー抽出(ASCII / 日本語 / `#` の前にサロゲートペアの絵文字がある場合の UTF-16 オフセット)、
前方一致フィルタ、`text.changed` → `fow.open` → `pick.0` → `composer.text.replace` + `storage.set` + `fow.close` の
一連の流れ、トリガーが消えたときの `fow.close` を検証している。

`cargo test` は native で走るので、wasm でだけ起きる問題(時刻 / 乱数 / スレッド、`docs/cli.md` §5 の落とし穴)は
**ビルド済みの wasm をそのまま叩く** `rasq call` で確かめる。ホスト関数の答えは `--reply`、
同じインスタンスで続けて呼ぶのは `--then`:

```sh
rasq call composer-text-plugin plugin.manifest

# 「🚀 #打」でカーソルが末尾(UTF-16 で 5)→ 候補 FOW → 先頭をタップ
rasq call composer-text-plugin text.changed \
  --args '{"token":"tk1","text":"🚀 #打","selectionStart":5,"selectionEnd":5}' \
  --reply 'storage.get={"value":["写真"]}' --reply 'storage.set={}' \
  --reply 'fow.open={"opened":true}' --reply 'fow.close={}' \
  --reply 'composer.text.replace={}' --reply 'toast={}' \
  --then pick.0
# text.changed → host storage.get {key:"recent", ns:"safe"}
#              → host fow.open {id:"hashtag-suggest", onClosed:"suggestClosed",
#                   root: ListTile "#打上会"(subtitle "辞書", on.tap "pick.0")+ note,
#                   spec: {title:"ハッシュタグ候補", initialHeightFactor:0.35, attachTop:false}}
# pick.0       → host composer.text.replace {token:"tk1", start:3, end:5, text:"#打上会 "}
#              → host storage.get / storage.set {key:"recent", value:["打上会","写真"]}
#              → host fow.close {id:"hashtag-suggest"} → host toast {message:"#打上会 を挿入しました"}

# トリガーが消えたら閉じる
rasq call composer-text-plugin text.changed \
  --args '{"token":"tk1","text":"plain","selectionStart":5,"selectionEnd":5}' --reply 'fow.close={}'

rasq call composer-text-plugin pick.x --args '{}'   # → C01(exit 1)
```

`start: 3 / end: 5` は `🚀`(サロゲートペア = 2 単位)+ 空白 1 単位の後ろ、つまり `#打` の範囲。
UTF-16 code unit で数えている(= Dart の `String` オフセット)ことがここで確かめられる。

## 端末での試し方(rasq-client)

1. `rasq dev`(または `rasq build --raw`)が出す `dist/` の絶対パスを控える。
2. サイドバー → **WASMプラグイン試験 (dev)** → `.wasm のパスからインポート` に `composer_text_plugin.wasm` の絶対パスを貼って **読み込み**。
3. **スロットに登録**(権限 `fow.widget` / `storage.safe` を許可)。
4. コンポーザーを開いて本文に `#` を打つ(例: `#打`)→ 200ms 後に候補 FOW が出る。
5. 候補をタップ → 本文の `#打` が `#打上会 ` に置き換わり、トーストが出る。次に `#` を打つとそのタグが先頭に出る。

## 掲載(審査)に出す

`rasq build` が出す `dist/sample.composer-text-0.1.0.rasqpkg` がそのまま掲載できる形
(zstd tar + `bundle.json` `rasq.bundle/1` + `SHA512SUMS`)。`rasq publish --central <url> --dry-run` で
Central への 3 要求を書き出せる。開発者鍵の署名(`*.sig.json`)はサイドロード用で、リポジトリには入れない。
