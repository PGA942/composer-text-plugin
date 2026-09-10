# composer-text-plugin — ハッシュタグ候補(サンプル)

スロット **`composer.text`**(CONTRACT.md §2.3)。コンポーザー本文で `#` を打つと、
**最近使ったタグ**(`storage.safe` の `recent`)+ **組み込み辞書** から前方一致の候補(最大 8 件)を
FOW に出し、タップで本文の `#トークン` を `#タグ ` に置き換える。

## call / ホスト関数の流れ

```
ホスト → plugin.manifest                       ← {id:"sample.composer-text", slots:["composer.text"], permissions:["fow.widget","storage.safe"]}
ホスト → text.changed {token, text, selectionStart, selectionEnd, trigger?}
            trigger が無ければ自分で判定: selectionStart から [^\s#@] を遡り、その直前が `#` なら
            `#<run>` がトリガー(範囲は UTF-16 code unit = Dart の String オフセット)
            トリガーあり → storage.get recent → fow.open {id:"hashtag-suggest", spec:{title:"ハッシュタグ候補", initialHeightFactor:0.35, attachTop:false}, onClosed:"suggestClosed", root:[ListTile × 候補]}
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

## ビルド / テスト

```sh
cargo test -p composer-text-plugin
cargo build --release --target wasm32-unknown-unknown -p composer-text-plugin
#  → target/wasm32-unknown-unknown/release/composer_text_plugin.wasm
```

native テストはトリガー抽出(ASCII / 日本語 / `#` の前にサロゲートペアの絵文字がある場合の UTF-16 オフセット)、
前方一致フィルタ、`text.changed` → `fow.open` → `pick.0` → `composer.text.replace` + `storage.set` + `fow.close` の
一連の流れ、トリガーが消えたときの `fow.close` を検証している。

## 端末で試す(rasq-client)

1. サイドバー → **WASMプラグイン試験 (dev)** → `.wasm のパスからインポート` に `composer_text_plugin.wasm` の絶対パス → **読み込み**
2. **スロットに登録**(権限 `fow.widget` / `storage.safe` を許可)
3. コンポーザーを開いて本文に `#` を打つ(例: `#打`)→ 200ms 後に候補 FOW が出る
4. 候補をタップ → 本文の `#打` が `#打上会 ` に置き換わり、トーストが出る。次に `#` を打つとそのタグが先頭に出る

---

## このリポジトリの形

- クレートはルート(`src/`)、共通部品 [`rasq-plugin-kit`](kit) は `kit/` に同梱(開発用モノレポから写している)。
- 契約は [`CONTRACT.md`](CONTRACT.md)(スロットプラグイン契約 v0.1)。
- ビルド済み `.wasm` は [`dist/`](dist)。`cargo build --release --target wasm32-unknown-unknown` で更新。
