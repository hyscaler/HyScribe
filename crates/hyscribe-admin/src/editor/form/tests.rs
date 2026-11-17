// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

fn rich(agent: &str, mobile: bool) -> bool {
    let browser = Browser {
        user_agent: agent.as_bytes(),
        mobile_hint: Some(if mobile { b"?1" } else { b"?0" }),
    };
    user_can_richedit(Some(b"true".to_vec()), &browser)
}

/// `user_can_richedit()` over the browsers `vars.php` tells apart.
#[test]
fn rich_editing_by_browser() {
    let safari = "Mozilla/5.0 (iPhone) AppleWebKit/533.1 Safari/533";
    for (agent, mobile, can) in [
        ("Lynx/2.8", false, false),
        ("Mozilla/5.0 Edg/120", false, true),
        (safari, false, true),
        (safari, true, false),
        (
            "Mozilla/5.0 (iPhone) AppleWebKit/605.1 Safari/604",
            true,
            true,
        ),
        (
            "Mozilla/5.0 (Windows NT 10.0; Trident/7.0; rv:11.0)",
            false,
            true,
        ),
        (
            "Mozilla/4.0 (compatible; MSIE 8.0; Windows NT 6.1)",
            false,
            false,
        ),
        (
            "Mozilla/5.0 (X11; rv:120.0) Gecko/20100101 Firefox/120.0",
            false,
            true,
        ),
        (
            "Mozilla/5.0 (Windows NT 10.0; rv:120.0) Gecko/20100101 Firefox/120.0",
            false,
            true,
        ),
        ("Opera/9.80 (X11)", false, true),
        ("Opera/9.80 (X11)", true, false),
        ("curl/8", false, false),
    ] {
        assert_eq!(rich(agent, mobile), can, "{agent} {mobile}");
    }
    let browser = Browser {
        user_agent: b"Mozilla/5.0 Chrome/140",
        mobile_hint: None,
    };
    assert!(!user_can_richedit(Some(b"false".to_vec()), &browser));
}
