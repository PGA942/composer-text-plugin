//! composer-text-plugin — コンポーザー本文の **ハッシュタグ候補**(スロット `composer.text`)のサンプル。
//!
//! | 項目 | 入口 |
//! |---|---|
//! | マニフェスト | `plugin.manifest` |
//! | 本文が変わった(デバウンス) | `text.changed {token, text, selectionStart, selectionEnd, trigger?}` |
//! | `#` トークンを検出 | [`find_trigger`](find_trigger)(UTF-16 code unit のオフセット = Dart の `String`) |
//! | 候補を出す | `fow.open`(同じ id で再要求 = 中身差し替え)、`ListTile` × 候補(最大 8) |
//! | 候補をタップ | `pick.<i>` → `composer.text.replace {token, start, end, text}` + `storage.safe: recent` 更新 + `fow.close` + `toast` |
//! | トリガー無し | 開いていれば `fow.close` |
//!
//! ビルド: `rasq check` / `rasq build --raw`(実 wasm を叩くなら `rasq call composer-text-plugin …`)。
//! 契約: v0.13(`rasq-sdk-sample/docs/slots.md` §2.3 / §4、[`../../CONTRACT.md`](../../CONTRACT.md))。
//!
//! `plugin.manifest` の答えは同じディレクトリの `manifest.json` そのもの(`include_str!`。
//! `rasq check` の往復検査が常に一致する)。

use rasq_plugin_kit::{fow_spec, host, i18n, rasq_plugin, rrl, serde_json::{json, Value}, unsupported, Surface};
use std::sync::Mutex;

mod text;
use text::TEXT;

/// 候補の上限 / 最近使ったタグの保存上限。
pub const MAX_CANDIDATES: usize = 8;
pub const MAX_RECENT: usize = 20;

/// 組み込み辞書(最近使ったタグの後ろに並ぶ)。
pub const DICTIONARY: [&str; 10] = ["打上会", "スペースライフ", "宇宙", "ロケット発射", "写真", "料理", "音楽", "ゲーム", "雑談", "RASQ"];

static PANEL: Surface = Surface::new("hashtag-suggest");

// ---------------------------------------------------------------------------
// 状態(開いている候補 FOW ぶん)
// ---------------------------------------------------------------------------

#[derive(Default)]
struct State {
    /// 直近の `text.changed` の token(`composer.text.replace` に返す)。
    token: String,
    /// トリガー `#…` の範囲(UTF-16 code unit、`[start, end)`)。
    start: usize,
    end: usize,
    candidates: Vec<String>,
    open: bool,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    f(guard.get_or_insert_with(State::default))
}

// ---------------------------------------------------------------------------
// トリガー検出(UTF-16 code unit)
// ---------------------------------------------------------------------------

/// カーソル直前の `#` トークン。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trigger {
    /// `#` を除いた入力中の文字列。
    pub query: String,
    /// `#` を含む範囲(UTF-16 code unit、Dart の `String` オフセット)。
    pub start: usize,
    pub end: usize,
}

/// UTF-16 の 1 単位がトークンを区切る文字か(空白 / `#` / `@`)。サロゲートは区切らない。
fn is_boundary_unit(u: u16) -> bool {
    match char::from_u32(u as u32) {
        Some(c) => c.is_whitespace() || c == '#' || c == '@',
        None => false, // サロゲート半分 = 絵文字などの途中
    }
}

/// `text` の `cursor`(UTF-16 code unit)直前にある `#トークン` を探す。
/// `cursor` は `text` の長さに丸める。`#` だけでも(`query` 空で)トリガーになる。
pub fn find_trigger(text: &str, cursor: usize) -> Option<Trigger> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let end = cursor.min(units.len());
    let mut i = end;
    while i > 0 && !is_boundary_unit(units[i - 1]) {
        i -= 1;
    }
    if i == 0 || units[i - 1] != u16::from(b'#') {
        return None;
    }
    let start = i - 1;
    Some(Trigger { query: String::from_utf16_lossy(&units[i..end]), start, end })
}

/// ホストが `trigger` を切り出して渡してきた場合に解釈する。形は
/// `"#タグ"`(文字列: 範囲はカーソル直前として復元)か `{text, start, end}`(オブジェクト)を受ける。
/// `#` で始まらないトークン(`@` / `:`)は `None`。
fn trigger_from_host(trigger: &Value, text: &str, cursor: usize) -> Option<Trigger> {
    let (tok, range) = match trigger {
        Value::String(s) => (s.as_str(), None),
        Value::Object(_) => {
            let s = trigger["text"].as_str()?;
            let r = match (trigger["start"].as_u64(), trigger["end"].as_u64()) {
                (Some(a), Some(b)) if a <= b => Some((a as usize, b as usize)),
                _ => None,
            };
            (s, r)
        }
        _ => return None,
    };
    let rest = tok.strip_prefix('#')?;
    let len16 = tok.encode_utf16().count();
    let (start, end) = match range {
        Some(r) => r,
        None => {
            let end = cursor.min(text.encode_utf16().count());
            (end.checked_sub(len16)?, end)
        }
    };
    Some(Trigger { query: rest.to_string(), start, end })
}

// ---------------------------------------------------------------------------
// 候補
// ---------------------------------------------------------------------------

/// 最近使ったタグ(新しい順)+ 辞書を、`query` の前方一致(大小文字を無視)で絞る。重複除去、最大 [`MAX_CANDIDATES`]。
pub fn candidates(recent: &[String], query: &str) -> Vec<String> {
    let q = query.to_lowercase();
    let mut out: Vec<String> = Vec::new();
    for tag in recent.iter().map(String::as_str).chain(DICTIONARY.iter().copied()) {
        if tag.is_empty() || !tag.to_lowercase().starts_with(&q) {
            continue;
        }
        if out.iter().any(|t| t == tag) {
            continue;
        }
        out.push(tag.to_string());
        if out.len() >= MAX_CANDIDATES {
            break;
        }
    }
    out
}

fn load_recent() -> Vec<String> {
    host::storage_get("recent")
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

/// `tag` を先頭に(重複は除き、最大 [`MAX_RECENT`])。
pub fn push_recent(recent: &mut Vec<String>, tag: &str) {
    recent.retain(|t| t != tag);
    recent.insert(0, tag.to_string());
    recent.truncate(MAX_RECENT);
}

// ---------------------------------------------------------------------------
// RRL(FOW の中身)
// ---------------------------------------------------------------------------

fn panel_root(loc: &str, cands: &[String], recent: &[String], query: &str) -> Value {
    let mut children: Vec<Value> = cands
        .iter()
        .enumerate()
        .map(|(i, tag)| {
            let subtitle = if recent.iter().any(|r| r == tag) { TEXT.t(loc, "panel.recent") } else { TEXT.t(loc, "panel.dictionary") };
            rrl::list_tile(&format!("cand-{i}"), &format!("#{tag}"), Some(subtitle), Some(&format!("pick.{i}")))
        })
        .collect();
    if cands.is_empty() {
        children.push(rrl::text("none", TEXT.f(loc, "panel.none", &[("query", query)])));
    }
    children.push(rrl::styled_text("note", TEXT.t(loc, "panel.note"), 11.0, false, Some("#888888")));
    rrl::scroll("root-scroll", false, vec![rrl::column("root", children)])
}

/// 本体ツリーは使わない(UI はコンポーザー横の FOW だけ)。
fn root() -> Value {
    rrl::column("root", vec![rrl::text("hello", TEXT.t(&i18n::current(), "root.hello"))])
}

// ---------------------------------------------------------------------------
// call
// ---------------------------------------------------------------------------

fn close_if_open() {
    with_state(|s| {
        if s.open {
            PANEL.close(json!({}));
        }
        *s = State::default();
    });
}

/// `plugin.manifest` の答え = `manifest.json`(名前 / 版 / 説明 / 権限はあちらを直す)。
fn manifest() -> Value {
    rasq_plugin_kit::serde_json::from_str(include_str!("../manifest.json")).expect("manifest.json")
}

fn handle(fn_name: &str, args: Value) -> Result<Value, Value> {
    match fn_name {
        "plugin.manifest" => Ok(manifest()),

        "text.changed" => {
            let token = args["token"].as_str().unwrap_or("").to_string();
            let text = args["text"].as_str().unwrap_or("");
            let cursor = args["selectionStart"].as_u64().unwrap_or(0) as usize;
            let trigger = if args["trigger"].is_null() {
                find_trigger(text, cursor)
            } else {
                trigger_from_host(&args["trigger"], text, cursor)
            };
            let Some(tr) = trigger else {
                close_if_open();
                return Ok(json!({}));
            };
            if token.is_empty() {
                close_if_open();
                return Ok(json!({}));
            }
            let recent = load_recent();
            let cands = candidates(&recent, &tr.query);
            let loc = i18n::locale_of(&args);
            let root = panel_root(loc, &cands, &recent, &tr.query);
            let mut spec = fow_spec(TEXT.t(loc, "panel.title"), 0.35);
            spec["attachTop"] = json!(false);
            let opened = PANEL.open(spec, root, Some("suggestClosed"), json!({}));
            with_state(|s| {
                *s = State { token, start: tr.start, end: tr.end, candidates: cands, open: opened };
            });
            Ok(json!({}))
        }

        f if f.starts_with("pick.") => {
            let i: usize = f["pick.".len()..].parse().map_err(|_| unsupported(f))?;
            let picked = with_state(|s| {
                if !s.open {
                    return None;
                }
                s.candidates.get(i).cloned().map(|tag| (s.token.clone(), s.start, s.end, tag))
            });
            let Some((token, start, end, tag)) = picked else {
                // 古い FOW からのタップなど: 何もしない
                return Ok(json!({}));
            };
            host::composer_text_replace(&token, start, end, &format!("#{tag} "));
            let mut recent = load_recent();
            push_recent(&mut recent, &tag);
            host::storage_set("recent", json!(recent));
            close_if_open();
            host::toast(&TEXT.f(i18n::locale_of(&args), "toast.inserted", &[("tag", &tag)]));
            Ok(json!({}))
        }

        // FOW がユーザー操作で閉じた(✕ / 余白タップ): 状態だけ捨てる(fow.close は呼ばない)
        "suggestClosed" => {
            with_state(|s| *s = State::default());
            Ok(json!({}))
        }
        "lifecycle" => Ok(json!({})),

        other => Err(unsupported(other)),
    }
}

rasq_plugin! { scope: "sample.composer-text:main", root: root, call: handle }

// ---------------------------------------------------------------------------
// テスト(native)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rasq_plugin_kit::{set_host_hook, take_host_log};
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::{Mutex as StdMutex, MutexGuard};

    static SERIAL: StdMutex<()> = StdMutex::new(());

    /// ホストのふり: fow.open は受理、storage.safe は `recent` を持つ。
    fn setup(recent: Vec<&str>) -> (MutexGuard<'static, ()>, Rc<RefCell<Value>>) {
        let g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let store = Rc::new(RefCell::new(json!(recent)));
        let st = store.clone();
        set_host_hook(move |f, a| match f {
            "fow.open" => json!({"opened": true}),
            "storage.get" if a["key"] == "recent" => json!({"value": st.borrow().clone()}),
            "storage.set" if a["key"] == "recent" => {
                *st.borrow_mut() = a["value"].clone();
                json!({})
            }
            _ => json!({}),
        });
        with_state(|s| *s = State::default());
        take_host_log();
        (g, store)
    }

    fn utf16_len(s: &str) -> usize {
        s.encode_utf16().count()
    }

    /// `plugin.manifest` は `manifest.json` そのもの(`rasq check` の往復検査と同じ突き合わせ)。
    #[test]
    fn manifest_matches_manifest_json() {
        let m = call_native("plugin.manifest", json!({})).unwrap();
        let file: Value = rasq_plugin_kit::serde_json::from_str(include_str!("../manifest.json")).unwrap();
        assert_eq!(m, file);
        assert_eq!(m["slots"], json!(["composer.text"]));
        assert_eq!(m["permissions"], json!(["fow.widget", "storage.safe"]));
    }

    #[test]
    fn trigger_ascii() {
        let t = find_trigger("hello #ra", 9).unwrap();
        assert_eq!(t, Trigger { query: "ra".into(), start: 6, end: 9 });
        // `#` だけ
        let t = find_trigger("hi #", 4).unwrap();
        assert_eq!(t, Trigger { query: "".into(), start: 3, end: 4 });
        // カーソルがトークンの途中
        let t = find_trigger("#rasq", 3).unwrap();
        assert_eq!(t, Trigger { query: "ra".into(), start: 0, end: 3 });
        // トリガー無し
        assert_eq!(find_trigger("hello world", 11), None);
        assert_eq!(find_trigger("#tag done", 9), None);
        assert_eq!(find_trigger("@name", 5), None);
        assert_eq!(find_trigger("", 0), None);
        // カーソルが範囲外でも落ちない
        assert_eq!(find_trigger("#ab", 100).map(|t| t.end), Some(3));
    }

    #[test]
    fn trigger_japanese_with_surrogate_pair_before_hash() {
        // 🚀 は UTF-16 で 2 単位。Dart のオフセットは文字数ではなく code unit 数。
        let text = "今日は🚀 #打上";
        let cursor = utf16_len(text); // = 3 + 2 + 1 + 3 = 9
        assert_eq!(cursor, 9);
        let t = find_trigger(text, cursor).unwrap();
        assert_eq!(t, Trigger { query: "打上".into(), start: 6, end: 9 });
        // 復元して確認: units[6..9] == "#打上"
        let units: Vec<u16> = text.encode_utf16().collect();
        assert_eq!(String::from_utf16_lossy(&units[t.start..t.end]), "#打上");
        // 絵文字がトークンの中にあっても区切りにならない
        let text2 = "#宇宙🌏";
        let t2 = find_trigger(text2, utf16_len(text2)).unwrap();
        assert_eq!(t2, Trigger { query: "宇宙🌏".into(), start: 0, end: 5 });
        // 絵文字の直後に # (空白なし) でも `#` は区切りなのでトリガーになる
        let text3 = "🎉#ro";
        let t3 = find_trigger(text3, utf16_len(text3)).unwrap();
        assert_eq!(t3, Trigger { query: "ro".into(), start: 2, end: 5 });
    }

    #[test]
    fn host_trigger_shapes() {
        let text = "ab #ta";
        assert_eq!(trigger_from_host(&json!("#ta"), text, 6), Some(Trigger { query: "ta".into(), start: 3, end: 6 }));
        assert_eq!(trigger_from_host(&json!({"text": "#ta", "start": 3, "end": 6}), text, 6), Some(Trigger { query: "ta".into(), start: 3, end: 6 }));
        assert_eq!(trigger_from_host(&json!("@ta"), text, 6), None);
        assert_eq!(trigger_from_host(&json!(":smile"), text, 6), None);
        assert_eq!(trigger_from_host(&json!(12), text, 6), None);
    }

    #[test]
    fn prefix_filter() {
        let recent = vec!["rasq会".to_string(), "写真".to_string()];
        let c = candidates(&recent, "");
        assert_eq!(c.len(), MAX_CANDIDATES);
        assert_eq!(c[0], "rasq会");
        assert_eq!(c[1], "写真");
        assert_eq!(c[2], "打上会");
        // 辞書と重複する「写真」は 1 回だけ
        assert_eq!(c.iter().filter(|t| *t == "写真").count(), 1);
        // 大小文字を無視
        assert_eq!(candidates(&recent, "RA"), vec!["rasq会", "RASQ"]);
        assert_eq!(candidates(&recent, "rasq"), vec!["rasq会", "RASQ"]);
        assert_eq!(candidates(&[], "打"), vec!["打上会"]);
        assert!(candidates(&[], "zzz").is_empty());
    }

    #[test]
    fn push_recent_dedupes_and_caps() {
        let mut r: Vec<String> = (0..MAX_RECENT).map(|i| format!("t{i}")).collect();
        push_recent(&mut r, "t5");
        assert_eq!(r[0], "t5");
        assert_eq!(r.len(), MAX_RECENT);
        push_recent(&mut r, "new");
        assert_eq!(r[0], "new");
        assert_eq!(r.len(), MAX_RECENT);
        assert!(!r.contains(&"t19".to_string()));
    }

    #[test]
    fn end_to_end_open_pick_close() {
        let (_g, store) = setup(vec!["写真"]);
        let text = "🚀 #打";
        let cursor = utf16_len(text); // 2 + 1 + 2 = 5
        let r = call_native("text.changed", json!({"token": "tk1", "text": text, "selectionStart": cursor, "selectionEnd": cursor})).unwrap();
        assert_eq!(r, json!({}));
        let log = take_host_log();
        let open = log.iter().find(|(f, _)| f == "fow.open").expect("fow.open");
        assert_eq!(open.1["id"], "hashtag-suggest");
        assert_eq!(open.1["spec"]["title"], "ハッシュタグ候補");
        assert_eq!(open.1["spec"]["initialHeightFactor"], 0.35);
        assert_eq!(open.1["spec"]["attachTop"], false);
        assert_eq!(open.1["onClosed"], "suggestClosed");
        let tiles = open.1["root"]["c"][0]["c"].as_array().unwrap();
        let cands: Vec<&str> = tiles.iter().filter(|n| n["t"] == "ListTile").map(|n| n["p"]["title"].as_str().unwrap()).collect();
        assert_eq!(cands, vec!["#打上会"]);
        assert_eq!(tiles[0]["on"]["tap"], "pick.0");
        assert!(tiles.iter().any(|n| n["k"] == "note"));

        // 候補をタップ
        let r = call_native("pick.0", json!({"nodeId": "sample.composer-text:main:cand-0", "kind": "tap"})).unwrap();
        assert_eq!(r, json!({}));
        let log = take_host_log();
        let rep = log.iter().find(|(f, _)| f == "composer.text.replace").expect("composer.text.replace");
        assert_eq!(rep.1, json!({"token": "tk1", "start": 3, "end": 5, "text": "#打上会 "}));
        let set = log.iter().find(|(f, _)| f == "storage.set").expect("storage.set");
        assert_eq!(set.1["key"], "recent");
        assert_eq!(set.1["value"], json!(["打上会", "写真"]));
        assert_eq!(*store.borrow(), json!(["打上会", "写真"]));
        assert!(log.iter().any(|(f, a)| f == "fow.close" && a["id"] == "hashtag-suggest"));
        let toast = log.iter().find(|(f, _)| f == "toast").expect("toast");
        assert_eq!(toast.1["message"], "#打上会 を挿入しました");

        // もう一度タップしても(閉じた後なので)何もしない
        call_native("pick.0", json!({})).unwrap();
        assert!(take_host_log().is_empty());

        // 次に # を打つと、今入れたタグが先頭(最近使ったタグ)に出る
        call_native("text.changed", json!({"token": "tk2", "text": "#", "selectionStart": 1, "selectionEnd": 1})).unwrap();
        let log = take_host_log();
        let open = log.iter().find(|(f, _)| f == "fow.open").unwrap();
        assert_eq!(open.1["root"]["c"][0]["c"][0]["p"]["title"], "#打上会");
        assert_eq!(open.1["root"]["c"][0]["c"][0]["p"]["subtitle"], "最近使ったタグ");
    }

    /// `args.locale` の言語で FOW の文とトーストを出す(無い言語は ja)。
    #[test]
    fn texts_follow_locale() {
        let (_g, _store) = setup(vec!["写真"]);
        let _ = i18n::take_misses();
        call_native("text.changed", json!({"token": "tk1", "text": "#", "selectionStart": 1, "selectionEnd": 1, "locale": "en"})).unwrap();
        let log = take_host_log();
        let open = log.iter().find(|(f, _)| f == "fow.open").unwrap();
        assert_eq!(open.1["spec"]["title"], "Hashtag suggestions");
        assert_eq!(open.1["root"]["c"][0]["c"][0]["p"]["subtitle"], "Recently used");
        assert_eq!(open.1["root"]["c"][0]["c"][1]["p"]["subtitle"], "Dictionary");
        call_native("pick.0", json!({"locale": "zh-Hant-TW"})).unwrap();
        let toast = take_host_log().into_iter().find(|(f, _)| f == "toast").unwrap();
        assert_eq!(toast.1["message"], "已插入 #写真");

        call_native("text.changed", json!({"token": "tk2", "text": "#zzz", "selectionStart": 4, "selectionEnd": 4, "locale": "ko"})).unwrap();
        let log = take_host_log();
        let open = log.iter().find(|(f, _)| f == "fow.open").unwrap();
        assert_eq!(open.1["spec"]["title"], "해시태그 추천");
        let none = open.1["root"]["c"][0]["c"].as_array().unwrap().iter().find(|n| n["k"] == "none").unwrap().clone();
        assert_eq!(none["p"]["text"], "'#zzz'에 해당하는 추천이 없습니다");

        call_native("text.changed", json!({"token": "tk3", "text": "#", "selectionStart": 1, "selectionEnd": 1, "locale": "zh-Hans"})).unwrap();
        let open = take_host_log().into_iter().find(|(f, _)| f == "fow.open").unwrap();
        assert_eq!(open.1["spec"]["title"], "话题标签建议");
        // 知らない言語は ja
        call_native("text.changed", json!({"token": "tk4", "text": "#", "selectionStart": 1, "selectionEnd": 1, "locale": "fr"})).unwrap();
        let open = take_host_log().into_iter().find(|(f, _)| f == "fow.open").unwrap();
        assert_eq!(open.1["spec"]["title"], "ハッシュタグ候補");
        assert!(i18n::take_misses().is_empty());
    }

    #[test]
    fn no_trigger_closes_open_fow() {
        let (_g, _store) = setup(vec![]);
        call_native("text.changed", json!({"token": "tk1", "text": "#ra", "selectionStart": 3, "selectionEnd": 3})).unwrap();
        assert!(take_host_log().iter().any(|(f, _)| f == "fow.open"));
        call_native("text.changed", json!({"token": "tk2", "text": "#ra ", "selectionStart": 4, "selectionEnd": 4})).unwrap();
        let log = take_host_log();
        assert!(log.iter().any(|(f, a)| f == "fow.close" && a["id"] == "hashtag-suggest"));
        assert!(!log.iter().any(|(f, _)| f == "fow.open"));
        // 閉じている時にトリガー無しなら fow.close は呼ばない
        call_native("text.changed", json!({"token": "tk3", "text": "plain", "selectionStart": 5, "selectionEnd": 5})).unwrap();
        assert!(take_host_log().is_empty());
    }

    #[test]
    fn host_supplied_trigger_is_used() {
        let (_g, _store) = setup(vec![]);
        call_native("text.changed", json!({"token": "tk1", "text": "x #写", "selectionStart": 4, "selectionEnd": 4, "trigger": "#写"})).unwrap();
        take_host_log();
        call_native("pick.0", json!({})).unwrap();
        let rep = take_host_log().into_iter().find(|(f, _)| f == "composer.text.replace").unwrap();
        assert_eq!(rep.1, json!({"token": "tk1", "start": 2, "end": 4, "text": "#写真 "}));
        // @ トークンは対象外 → 閉じる
        call_native("text.changed", json!({"token": "tk2", "text": "@me", "selectionStart": 3, "selectionEnd": 3, "trigger": "@me"})).unwrap();
        assert!(!take_host_log().iter().any(|(f, _)| f == "fow.open"));
    }

    #[test]
    fn user_closed_then_pick_is_noop() {
        let (_g, _store) = setup(vec![]);
        call_native("text.changed", json!({"token": "tk1", "text": "#", "selectionStart": 1, "selectionEnd": 1})).unwrap();
        call_native("suggestClosed", json!({"id": "hashtag-suggest", "reason": "cancel"})).unwrap();
        take_host_log();
        call_native("pick.0", json!({})).unwrap();
        assert!(take_host_log().is_empty());
    }

    #[test]
    fn bad_input_does_not_panic() {
        let (_g, _store) = setup(vec![]);
        assert!(call_native("text.changed", json!(null)).is_ok());
        assert!(call_native("text.changed", json!({"text": "#a", "selectionStart": -5})).is_ok());
        assert!(call_native("pick.99", json!(null)).is_ok());
        assert!(call_native("pick.x", json!(null)).is_err());
        assert!(call_native("lifecycle", json!({"kind": "mount"})).is_ok());
        assert!(call_native("nope", json!({})).is_err());
    }
}
