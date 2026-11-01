//! Default workspace names: short, memorable, lowercase city names.

pub const CITIES: &[&str] = &[
    "hyderabad",
    "vizag",
    "warangal",
    "tirupati",
    "nellore",
    "guntur",
    "kakinada",
    "karimnagar",
    "lisbon",
    "porto",
    "seville",
    "valencia",
    "lyon",
    "nantes",
    "turin",
    "verona",
    "bruges",
    "ghent",
    "utrecht",
    "aarhus",
    "bergen",
    "tallinn",
    "riga",
    "vilnius",
    "krakow",
    "gdansk",
    "brno",
    "graz",
    "zagreb",
    "split",
    "ljubljana",
    "sofia",
    "tbilisi",
    "yerevan",
    "baku",
    "muscat",
    "doha",
    "kochi",
    "mysore",
    "pune",
    "jaipur",
    "udaipur",
    "shillong",
    "gangtok",
    "kandy",
    "galle",
    "hanoi",
    "hue",
    "penang",
    "ipoh",
    "cebu",
    "osaka",
    "kyoto",
    "nara",
    "sapporo",
    "busan",
    "taipei",
    "tainan",
    "perth",
    "hobart",
    "nelson",
    "dunedin",
    "quito",
    "cusco",
    "lima",
    "bogota",
    "medellin",
    "oaxaca",
    "merida",
    "havana",
    "austin",
    "boulder",
    "portland",
    "halifax",
    "quebec",
    "nairobi",
    "kigali",
    "accra",
    "dakar",
    "marrakesh",
    "fes",
    "tunis",
    "cairo",
    "luxor",
    "petra",
    "izmir",
    "bursa",
    "odessa",
    "kyiv",
    "minsk",
];

/// First city not in `taken`; falls back to `city-N` once all are used.
pub fn pick(taken: &[String]) -> String {
    let is_taken = |n: &str| taken.iter().any(|t| t == n);
    if let Some(c) = CITIES.iter().find(|c| !is_taken(c)) {
        return c.to_string();
    }
    (2..)
        .flat_map(|n| CITIES.iter().map(move |c| format!("{c}-{n}")))
        .find(|n| !is_taken(n))
        .expect("infinite iterator")
}

/// Workspace names become branch and directory names, so keep them boring.
pub fn is_valid(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 48
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-')
}
