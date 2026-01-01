use phf::phf_map;

static LANG_MAP: phf::Map<&'static str, &'static str> = phf_map! {
    "eng" => "en",
    "deu" => "de",
    "ger" => "de",
    "spa" => "es",
    "fra" => "fr",
    "fre" => "fr",
    "ita" => "it",
    "jpn" => "ja",
    "por" => "pt",
    "rus" => "ru",
    "hun" => "hu",
    "pol" => "pl",
    "ces" => "cs",
    "cze" => "cs",
    "slo" => "sk",
    "slk" => "sk",


};

pub fn normalize_lang(lang: &str) -> Option<&'static str> {
    LANG_MAP.get(lang).map(|l| *l)
}
