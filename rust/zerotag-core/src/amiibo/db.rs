// SPDX-License-Identifier: MIT
//! Amiibo catalogue (port of src/amiibo/amiibo_db.cpp): loading, online merge, accent-insensitive search, facets.

use super::image::parse_id;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// One entry of assets/amiibo/amiibo_db.json (data from AmiiboAPI).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    /// 16 hex characters: head (8) + tail (8), lower case.
    pub id: String,
    pub name: String,
    pub character: String,
    pub game_series: String,
    pub amiibo_series: String,
    pub type_: String,
    /// Latest regional release date "YYYY-MM-DD" ("" when unknown); only from the online list.
    pub release: String,
    /// Not in the bundled catalogue: learnt from the online update.
    pub online: bool,
    pub head: u32,
    pub tail: u32,
}

/// Fields the catalogue can be browsed by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facet {
    AmiiboSeries,
    Game,
    Type,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacetCount {
    pub value: String,
    pub count: usize,
}

#[derive(Debug, Default)]
pub struct Database {
    entries: Vec<Entry>,
}

fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

/// Lower-case and drop the accents of Latin-1 letters: "Pokémon" matches "pokemon" and the other way round.
fn fold(s: &str) -> String {
    const MAP: &[u8; 32] = b"aaaaaaaceeeeiiiidnooooo/ouuuuypy"; // U+00E0..U+00FF
    s.chars()
        .map(|c| match c as u32 {
            cp @ 0xC0..=0xDF => MAP[(cp + 0x20 - 0xE0) as usize] as char,
            cp @ 0xE0..=0xFF => MAP[(cp - 0xE0) as usize] as char,
            _ => c.to_ascii_lowercase(),
        })
        .collect()
}

fn words_of(query: &str) -> Vec<String> {
    fold(query).split_whitespace().map(str::to_owned).collect()
}

fn matches(e: &Entry, words: &[String]) -> bool {
    let hay = fold(&format!("{}\n{}\n{}\n{}\n{}", e.name, e.character, e.game_series, e.amiibo_series, e.id));
    words.iter().all(|w| hay.contains(w.as_str()))
}

fn facet_value(e: &Entry, facet: Facet) -> &str {
    match facet {
        Facet::AmiiboSeries => &e.amiibo_series,
        Facet::Game => &e.game_series,
        Facet::Type => &e.type_,
    }
}

fn str_field(o: &serde_json::Map<String, Value>, key: &str) -> String {
    o.get(key).and_then(Value::as_str).unwrap_or("").to_owned()
}

/// The bundled catalogue has "release" absent; AmiiboAPI has {"au": "2026-11-12" | null, ...}: keep the latest date.
fn release_of(o: &serde_json::Map<String, Value>) -> String {
    match o.get("release") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Object(m)) => m.values().filter_map(Value::as_str).max().unwrap_or("").to_owned(),
        _ => String::new(),
    }
}

/// The bundled catalogue has "id"; the raw AmiiboAPI answer has "head" and "tail" ("0x..." strings).
fn entry_from(o: &serde_json::Map<String, Value>) -> Option<Entry> {
    let mut id = lower(&str_field(o, "id"));
    if id.is_empty() {
        let strip = |s: String| s.strip_prefix("0x").map(str::to_owned).unwrap_or(s);
        id = strip(lower(&str_field(o, "head"))) + &strip(lower(&str_field(o, "tail")));
    }
    let (head, tail) = parse_id(&id)?;
    Some(Entry {
        id,
        name: str_field(o, "name"),
        character: str_field(o, "character"),
        game_series: str_field(o, "gameSeries"),
        amiibo_series: str_field(o, "amiiboSeries"),
        type_: str_field(o, "type"),
        release: release_of(o),
        online: false,
        head,
        tail,
    })
}

fn read_entries(path: &Path) -> Result<Vec<Entry>, String> {
    let text = std::fs::read(path).map_err(|_| format!("cannot open {}", path.display()))?;
    parse_entries(&text)
}

fn parse_entries(text: &[u8]) -> Result<Vec<Entry>, String> {
    let v: Value = serde_json::from_slice(text).map_err(|e| format!("amiibo_db.json: {e}"))?;
    let top = v.as_object().ok_or("amiibo_db.json: top level is not an object")?;
    let list = match top.get("amiibo") {
        Some(Value::Array(a)) => a.as_slice(),
        _ => &[],
    };
    Ok(list.iter().filter_map(|x| x.as_object()).filter_map(entry_from).collect())
}

impl Database {
    /// Loads the catalogue. Entries with an invalid id are skipped.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let entries = read_entries(path)?;
        if entries.is_empty() {
            return Err(format!("no amiibo entry found in {}", path.display()));
        }
        Ok(Self { entries })
    }

    pub fn from_json(text: &[u8]) -> Result<Self, String> {
        Ok(Self { entries: parse_entries(text)? })
    }

    /// Adds the entries of an AmiiboAPI answer that are not in the catalogue yet, flagged `online`, and fills in
    /// release dates. Returns how many entries were added.
    pub fn merge(&mut self, path: impl AsRef<Path>) -> Result<usize, String> {
        let incoming = read_entries(path.as_ref())?;
        Ok(self.merge_entries(incoming))
    }

    pub fn merge_entries(&mut self, incoming: Vec<Entry>) -> usize {
        let mut index: HashMap<String, usize> =
            self.entries.iter().enumerate().map(|(i, e)| (e.id.clone(), i)).collect();
        let mut added = 0;
        for mut e in incoming {
            if let Some(&i) = index.get(&e.id) {
                // Known entry: keep the bundled text, learn the release date.
                if self.entries[i].release.is_empty() {
                    self.entries[i].release = e.release;
                }
                continue;
            }
            e.online = true;
            index.insert(e.id.clone(), self.entries.len());
            self.entries.push(e);
            added += 1;
        }
        self.entries.sort_by(|a, b| {
            (&a.amiibo_series, &a.name, &a.id).cmp(&(&b.amiibo_series, &b.name, &b.id))
        });
        added
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn find(&self, head: u32, tail: u32) -> Option<&Entry> {
        self.entries.iter().find(|e| e.head == head && e.tail == tail)
    }

    /// Case- and accent-insensitive substring search over name, character, game series, amiibo series and the
    /// hex id. Every whitespace-separated word of `query` has to match. An empty query returns everything.
    pub fn search(&self, query: &str) -> Vec<&Entry> {
        let words = words_of(query);
        self.entries.iter().filter(|e| matches(e, &words)).collect()
    }

    /// Distinct non-empty values of `facet`, sorted case-insensitively, with their entry counts.
    pub fn facet_counts(&self, facet: Facet) -> Vec<FacetCount> {
        // Keyed by the lower-cased value: "Mario" and "mario" are the same value; the first spelling is shown.
        let mut by_key: BTreeMap<String, FacetCount> = BTreeMap::new();
        for e in &self.entries {
            let v = facet_value(e, facet);
            if v.is_empty() {
                continue;
            }
            let fc = by_key.entry(lower(v)).or_insert_with(|| FacetCount { value: v.to_owned(), count: 0 });
            fc.count += 1;
        }
        by_key.into_values().collect()
    }

    /// Entries whose `facet` equals `value` (case-insensitive, exact) and that match `query` as `search` does.
    pub fn search_in(&self, facet: Facet, value: &str, query: &str) -> Vec<&Entry> {
        let wanted = lower(value);
        if wanted.is_empty() {
            return Vec::new();
        }
        let words = words_of(query);
        self.entries.iter().filter(|e| lower(facet_value(e, facet)) == wanted && matches(e, &words)).collect()
    }

    /// `<images_dir>/<id>.png`
    pub fn image_path(images_dir: &str, entry: &Entry) -> String {
        format!("{}/{}.png", images_dir, entry.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"amiibo":[
      {"id":"01010000000e0002","name":"Link","character":"Link","gameSeries":"The Legend of Zelda","amiiboSeries":"Super Smash Bros.","type":"Figure"},
      {"id":"0000000002380602","name":"Pokémon Test","character":"Pika","gameSeries":"Pokémon","amiiboSeries":"pokemon","type":"Card"},
      {"id":"zz","name":"bad id"},
      {"id":"1111111111111111","name":"Other","gameSeries":"pokemon","amiiboSeries":"Pokemon","type":"Card"}]}"#;

    fn db() -> Database {
        Database::from_json(SAMPLE.as_bytes()).unwrap()
    }

    #[test]
    fn skips_invalid_ids() {
        assert_eq!(db().entries().len(), 3);
    }

    #[test]
    fn find_by_head_tail() {
        assert_eq!(db().find(0x01010000, 0x000e0002).unwrap().name, "Link");
    }

    #[test]
    fn search_is_accent_and_case_insensitive() {
        let d = db();
        assert_eq!(d.search("POKEMON").len(), 2);
        assert_eq!(d.search("pokémon test").len(), 1);
        assert_eq!(d.search("").len(), 3);
        assert_eq!(d.search("zelda link").len(), 1);
        assert_eq!(d.search("01010000000e").len(), 1); // hex id
    }

    #[test]
    fn facets_merge_case_and_sort() {
        let f = db().facet_counts(Facet::AmiiboSeries);
        let values: Vec<_> = f.iter().map(|x| (x.value.as_str(), x.count)).collect();
        assert_eq!(values, vec![("pokemon", 2), ("Super Smash Bros.", 1)]);
        assert_eq!(db().search_in(Facet::Type, "card", "").len(), 2);
        assert!(db().search_in(Facet::Type, "", "").is_empty());
    }

    #[test]
    fn merge_adds_unknown_and_learns_release() {
        let mut d = db();
        let online = br#"{"amiibo":[
          {"head":"0x01010000","tail":"0x000e0002","name":"X","release":{"na":"2014-11-21","eu":"2015-01-01","jp":null}},
          {"head":"0x09990000","tail":"0x00010002","name":"Brand New","amiiboSeries":"Future","release":{"na":"2099-01-01"}}]}"#;
        let incoming = parse_entries(online).unwrap();
        assert_eq!(d.merge_entries(incoming), 1);
        assert_eq!(d.find(0x01010000, 0x000e0002).unwrap().release, "2015-01-01");
        assert_eq!(d.find(0x01010000, 0x000e0002).unwrap().name, "Link");
        let n = d.find(0x09990000, 0x00010002).unwrap();
        assert!(n.online);
        assert_eq!(n.release, "2099-01-01");
    }

    #[test]
    fn bundled_catalogue_loads() {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/amiibo/amiibo_db.json");
        let d = Database::load(p).unwrap();
        assert_eq!(d.entries().len(), 954);
        assert!(d.find(0x01010000, 0x000e0002).is_some());
    }
}
