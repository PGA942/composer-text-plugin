//! 利用者に見せる文の表(PROTOCOL.md §4.6)。ホストの `args.locale`(無ければ `ja`)で引く。
//!
//! 基準は `ja`。`en` / `zh-Hans` / `zh-Hant` / `ko` は同じキーを全部持つ(テストで確かめる)。

rasq_plugin_kit::i18n_catalog! {
    pub(crate) static TEXT = {
        "ja": {
            "panel.title" => "ハッシュタグ候補",
            "panel.recent" => "最近使ったタグ",
            "panel.dictionary" => "辞書",
            "panel.none" => "「#{query}」に合う候補はありません",
            "panel.note" => "タップで本文の # トークンを置き換えます",
            "root.hello" => "ハッシュタグ候補(UI はコンポーザーの FOW)",
            "toast.inserted" => "#{tag} を挿入しました",
        },
        "en": {
            "panel.title" => "Hashtag suggestions",
            "panel.recent" => "Recently used",
            "panel.dictionary" => "Dictionary",
            "panel.none" => "No suggestions match “#{query}”",
            "panel.note" => "Tap to replace the # token in your text",
            "root.hello" => "Hashtag suggestions (shown in a panel next to the composer)",
            "toast.inserted" => "Inserted #{tag}",
        },
        "zh-Hans": {
            "panel.title" => "话题标签建议",
            "panel.recent" => "最近使用的标签",
            "panel.dictionary" => "词典",
            "panel.none" => "没有与“#{query}”匹配的建议",
            "panel.note" => "点按即可替换正文中的 # 标记",
            "root.hello" => "话题标签建议（显示在编辑器旁的浮动窗口中）",
            "toast.inserted" => "已插入 #{tag}",
        },
        "zh-Hant": {
            "panel.title" => "主題標籤建議",
            "panel.recent" => "最近使用的標籤",
            "panel.dictionary" => "字典",
            "panel.none" => "沒有符合「#{query}」的建議",
            "panel.note" => "點按即可取代內文中的 # 標記",
            "root.hello" => "主題標籤建議（顯示在編輯器旁的浮動視窗中）",
            "toast.inserted" => "已插入 #{tag}",
        },
        "ko": {
            "panel.title" => "해시태그 추천",
            "panel.recent" => "최근 사용한 태그",
            "panel.dictionary" => "사전",
            "panel.none" => "'#{query}'에 해당하는 추천이 없습니다",
            "panel.note" => "탭하면 본문의 # 토큰을 바꿉니다",
            "root.hello" => "해시태그 추천(작성 화면 옆의 플로팅 창에 표시)",
            "toast.inserted" => "#{tag} 태그를 삽입했습니다",
        },
    };
}

#[cfg(test)]
mod tests {
    use super::TEXT;
    use std::collections::BTreeSet;

    /// この表の言語ごとのキー(`Catalog::check` は「基準に無いキー」と差し込みの食い違いしか見ないので、訳の抜けはここで見る)。
    fn table_keys() -> Vec<(String, BTreeSet<String>)> {
        let mut out: Vec<(String, BTreeSet<String>)> = Vec::new();
        for line in include_str!("text.rs").lines().map(str::trim) {
            if let Some(tag) = line.strip_prefix('"').and_then(|l| l.strip_suffix("\": {")) {
                out.push((tag.to_string(), BTreeSet::new()));
            } else if let Some((key, _)) = line.strip_prefix('"').and_then(|l| l.split_once("\" =>")) {
                out.last_mut().expect("表の外のキー").1.insert(key.to_string());
            }
        }
        out
    }

    #[test]
    fn every_locale_has_every_key() {
        assert!(TEXT.check().is_empty(), "{:?}", TEXT.check());
        let tables = table_keys();
        let tags: Vec<&str> = tables.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(tags, TEXT.locales());
        assert_eq!(tags, ["ja", "en", "zh-Hans", "zh-Hant", "ko"]);
        for (tag, keys) in &tables[1..] {
            let missing: Vec<_> = tables[0].1.difference(keys).collect();
            assert!(missing.is_empty(), "{tag} に訳が無いキー: {missing:?}");
        }
    }
}
