use base64::Engine;
use tracked::riot::auth::{client_platform_header, PublicAuth, CLIENT_PLATFORM_JSON};
use tracked::riot::region::{
    from_launch_args, is_riot_api_host, is_valid_glz_host, region_from_launch_args, shard_from_launch_args,
};
use tracked::riot::{Hosts, Region, Shard};
use tracked::secret::Secret;

#[test]
fn platform_header_decodes_to_the_exact_json() {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(client_platform_header())
        .unwrap();
    assert_eq!(std::str::from_utf8(&raw).unwrap(), CLIENT_PLATFORM_JSON);
    assert!(!client_platform_header().contains('\n'));
    let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(v["platformType"], "PC");
    assert_eq!(v["platformOS"], "Windows");
    assert_eq!(v["platformOSVersion"], "10.0.19043.1.256.64bit");
    assert_eq!(v["platformChipset"], "Unknown");
}

#[test]
fn public_auth_sets_all_four_headers_and_hides_tokens() {
    let auth = PublicAuth::new(
        Secret::new("tok123".into()),
        Secret::new("jwt456".into()),
        "release-09.05-shipping-21-2843553".into(),
    );
    let h = auth.headers().unwrap();
    assert_eq!(h["authorization"], "Bearer tok123");
    assert_eq!(h["x-riot-entitlements-jwt"], "jwt456");
    assert_eq!(h["x-riot-clientversion"], "release-09.05-shipping-21-2843553");
    assert_eq!(h["x-riot-clientplatform"], client_platform_header().as_str());
    assert!(h["authorization"].is_sensitive());
    assert!(h["x-riot-entitlements-jwt"].is_sensitive());
    let dbg = format!("{auth:?} {h:?}");
    assert!(!dbg.contains("tok123") && !dbg.contains("jwt456"), "{dbg}");

    // apply() puts the same four on a request.
    let req = auth
        .apply(reqwest::Client::new().get("https://pd.na.a.pvp.net/x"))
        .build()
        .unwrap();
    for name in [
        "authorization",
        "x-riot-entitlements-jwt",
        "x-riot-clientplatform",
        "x-riot-clientversion",
    ] {
        assert!(req.headers().contains_key(name), "{name}");
    }
}

#[test]
fn region_to_shard_mapping_and_hosts() {
    let cases = [
        (Region::Na, Shard::Na),
        (Region::Latam, Shard::Na),
        (Region::Br, Shard::Na),
        (Region::Eu, Shard::Eu),
        (Region::Ap, Shard::Ap),
        (Region::Kr, Shard::Kr),
    ];
    for (region, shard) in cases {
        assert_eq!(region.default_shard(), shard);
        let h = Hosts::new(region, shard).unwrap();
        assert_eq!(h.pd, format!("https://pd.{shard}.a.pvp.net"));
        assert_eq!(h.glz, format!("https://glz-{region}-1.{shard}.a.pvp.net"));
        assert!(is_valid_glz_host(h.glz.trim_start_matches("https://")));
    }
    // NA can also be on the PBE shard; other combinations are rejected.
    assert_eq!(
        Hosts::new(Region::Na, Shard::Pbe).unwrap().glz,
        "https://glz-na-1.pbe.a.pvp.net"
    );
    assert!(Hosts::new(Region::Eu, Shard::Na).is_err());
    assert!(Hosts::new(Region::Latam, Shard::Eu).is_err());
    assert_eq!(Region::parse(" EU "), Some(Region::Eu));
    assert_eq!(Region::parse("mars"), None);
    assert_eq!(Region::parse("na.evil.com/"), None);
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
fn region_from_launch_arguments() {
    let args = [
        "-foo",
        "-ares-deployment=AP",
        "-config-endpoint=https://shared.ap.a.pvp.net",
    ];
    assert_eq!(region_from_launch_args(&args).as_deref(), Some("ap"));
    assert_eq!(from_launch_args(&args), Some((Region::Ap, Shard::Ap)));
    let latam = ["-ares-deployment=latam", "-config-endpoint=https://shared.na.a.pvp.net"];
    assert_eq!(from_launch_args(&latam), Some((Region::Latam, Shard::Na)));
    let pbe = ["-ares-deployment=pbe", "-config-endpoint=https://shared.pbe.a.pvp.net"];
    assert_eq!(from_launch_args(&pbe), Some((Region::Na, Shard::Pbe)));
    let only_endpoint = ["-config-endpoint=https://shared.kr.a.pvp.net"];
    assert_eq!(from_launch_args(&only_endpoint), Some((Region::Kr, Shard::Kr)));
    assert_eq!(from_launch_args(&["-x"]), None);
    assert_eq!(shard_from_launch_args(&["-config-endpoint=https://evil.com"]), None);
}

#[test]
fn region_hints_cover_riot_naming() {
    for (hint, want) in [
        ("NA", Region::Na),
        ("na1", Region::Na),
        ("EUW", Region::Eu),
        ("eune1", Region::Eu),
        ("eu3", Region::Eu),
        ("TR", Region::Eu),
        ("OC1", Region::Ap),
        ("jp1", Region::Ap),
        ("KR", Region::Kr),
        ("br1", Region::Br),
        ("LA1", Region::Latam),
        ("las", Region::Latam),
    ] {
        assert_eq!(Region::from_hint(hint), Some(want), "{hint}");
    }
    assert_eq!(Region::from_hint(""), None);
    assert_eq!(Region::from_hint("mars"), None);
}

#[test]
fn ci_version_from_log() {
    let log = "[2026.10.01] LogInit: Build: ++Ares-Core+release-09.05\n[..] LogShooter: Display: CI server version: release-09.05-shipping-21-2843553\n";
    assert_eq!(
        tracked::riot::parse_ci_version(log).as_deref(),
        Some("release-09.05-shipping-21-2843553")
    );
    assert_eq!(tracked::riot::parse_ci_version("nothing"), None);
}
