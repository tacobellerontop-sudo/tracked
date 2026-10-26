use ascend_lite::riot::auth::{client_platform_header, RiotAuth};
use ascend_lite::riot::region::{is_riot_api_host, is_valid_glz_host, region_from_launch_args, Region};
use ascend_lite::secret::Secret;
use base64::Engine;

#[test]
fn platform_header_decodes_to_expected_json() {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(client_platform_header())
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(v["platformType"], "PC");
    assert_eq!(v["platformOS"], "Windows");
    assert_eq!(v["platformOSVersion"], "10.0.19043.1.256.64bit");
    assert_eq!(v["platformChipset"], "Unknown");
}

#[test]
fn builds_all_four_headers_and_hides_tokens() {
    let auth = RiotAuth {
        access_token: Secret::new("tok123".into()),
        entitlements_jwt: Secret::new("jwt456".into()),
        puuid: "p".into(),
        client_version: "release-09.05-shipping-21-2843553".into(),
    };
    let h = auth.headers().unwrap();
    assert_eq!(h["authorization"], "Bearer tok123");
    assert_eq!(h["x-riot-entitlements-jwt"], "jwt456");
    assert_eq!(h["x-riot-clientversion"], "release-09.05-shipping-21-2843553");
    assert_eq!(h["x-riot-clientplatform"], client_platform_header().as_str());
    assert!(h["authorization"].is_sensitive());
    assert!(h["x-riot-entitlements-jwt"].is_sensitive());
    let dbg = format!("{auth:?} {h:?}");
    assert!(!dbg.contains("tok123") && !dbg.contains("jwt456"), "{dbg}");
}

#[test]
fn region_shard_mapping_and_hosts() {
    let cases = [
        ("na", "na"),
        ("latam", "na"),
        ("br", "na"),
        ("eu", "eu"),
        ("ap", "ap"),
        ("kr", "kr"),
    ];
    for (region, shard) in cases {
        let r = Region::new(region).unwrap();
        assert_eq!(r.shard, shard);
        assert_eq!(r.pd_base().unwrap(), format!("https://pd.{shard}.a.pvp.net"));
        assert_eq!(
            r.glz_base().unwrap(),
            format!("https://glz-{region}-1.{shard}.a.pvp.net")
        );
    }
    assert_eq!(Region::new(" EU ").unwrap().region, "eu");
    assert!(Region::new("mars").is_err());
    assert!(Region::new("").is_err());
    assert!(Region::new("na.evil.com/").is_err());
}

#[test]
fn glz_regex_validation() {
    assert!(is_valid_glz_host("glz-na-1.na.a.pvp.net"));
    assert!(is_valid_glz_host("glz-latam-1.na.a.pvp.net"));
    assert!(!is_valid_glz_host("glz-na-1.na.a.pvp.net.evil.com"));
    assert!(!is_valid_glz_host("evil.com/glz-na-1.na.a.pvp.net"));
    assert!(!is_valid_glz_host("glz-NA-1.na.a.pvp.net"));
    assert!(!is_valid_glz_host("glz-na-x.na.a.pvp.net"));
    assert!(!is_valid_glz_host("glz--1.na.a.pvp.net"));
}

#[test]
fn auth_headers_only_for_riot_hosts() {
    assert!(is_riot_api_host("pd.eu.a.pvp.net"));
    assert!(is_riot_api_host("glz-eu-1.eu.a.pvp.net"));
    assert!(!is_riot_api_host("valorant-api.com"));
    assert!(!is_riot_api_host("pd.eu.a.pvp.net.attacker.io"));
    assert!(!is_riot_api_host("127.0.0.1"));
}

#[test]
fn region_from_args() {
    let args = [
        "-foo",
        "-ares-deployment=AP",
        "-config-endpoint=https://shared.ap.a.pvp.net",
    ];
    assert_eq!(region_from_launch_args(&args).as_deref(), Some("ap"));
    assert_eq!(region_from_launch_args(&["-x"]), None);
}

#[test]
fn ci_version_from_log() {
    let log = "[2026.10.01] LogInit: Build: ++Ares-Core+release-09.05\n[..] LogShooter: Display: CI server version: release-09.05-shipping-21-2843553\n";
    assert_eq!(
        ascend_lite::riot::parse_ci_version(log).as_deref(),
        Some("release-09.05-shipping-21-2843553")
    );
    assert_eq!(ascend_lite::riot::parse_ci_version("nothing"), None);
}

#[test]
fn region_hints_cover_riot_naming() {
    use ascend_lite::riot::region::{region_from_hint, shard_from_launch_args};
    for (hint, want) in [
        ("NA", "na"),
        ("na1", "na"),
        ("EUW", "eu"),
        ("eune1", "eu"),
        ("eu3", "eu"),
        ("TR", "eu"),
        ("OC1", "ap"),
        ("jp1", "ap"),
        ("KR", "kr"),
        ("br1", "br"),
        ("LA1", "latam"),
        ("las", "latam"),
    ] {
        assert_eq!(region_from_hint(hint).as_deref(), Some(want), "{hint}");
    }
    assert_eq!(region_from_hint(""), None);
    assert_eq!(region_from_hint("mars"), None);
    let args = ["-x", "-config-endpoint=https://shared.eu.a.pvp.net"];
    assert_eq!(shard_from_launch_args(&args).as_deref(), Some("eu"));
    assert_eq!(shard_from_launch_args(&["-config-endpoint=https://evil.com"]), None);
}
