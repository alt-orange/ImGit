//! The Commit Signing page.

use dear_imgui_rs::InputTextFlags;

use super::controls::{BTN_34, Field, Tone};
use super::form::{Form, git_checkbox, git_field, help, row, title};
use super::page::{BoxStyle, Page};
use super::store::{
    Scope, generate_gpg_key, git_value, refresh_signing_keys, set_git_value, set_signing_key,
    signing_key_option,
};

pub(super) fn commit_signing_page(page: &mut Page<'_, '_>) {
    let mut form = Form::new(page);
    let form = &mut form;
    title(page, form, "Commit Signing");
    row(page, form, "GPG Format", |page, r| {
        let format = git_value(page.app, Scope::Global, "gpg.format").to_ascii_uppercase();
        let shown = if format.is_empty() {
            "OPENPGP"
        } else {
            &format
        };
        page.native_select(
            "##gpg_format",
            r.x(),
            r.top,
            300.0,
            shown,
            &["OPENPGP", "X509", "SSH"],
            |_, app, option| {
                let value = option.to_ascii_lowercase();
                // openpgp is git's default: the key is removed.
                let value = if value == "openpgp" { "" } else { &value };
                set_git_value(app, Scope::Global, "gpg.format", value);
                refresh_signing_keys(app);
            },
        );
        r.column = 34.0;
        help(
            page,
            r,
            "Sets gpg.format in your global Git Config. SSH signs with an SSH public key file.",
        );
    });
    row(page, form, "GPG Program", |page, r| {
        let field = Field::default().placeholder("gpg");
        git_field(
            page,
            r,
            "##gpg_program",
            Scope::Global,
            "gpg.program",
            &field,
            227.28,
        );
        if page.button(
            "##browse_gpg",
            r.x() + 232.28,
            r.top,
            "Browse",
            Tone::Primary,
            BTN_34,
            false,
        ) && let Some(path) =
            imgit_platform::shell::open_file_dialog(Some(page.cx.shell.window), Some("exe"))
        {
            set_git_value(
                page.app,
                Scope::Global,
                "gpg.program",
                &path.to_string_lossy(),
            );
            refresh_signing_keys(page.app);
        }
        help(
            page,
            r,
            "This is usually \"gpg\" or \"gpg2\" depending on the\nversion you are using.\nYou can also browse \
             for the path directly. Sets gpg.program.",
        );
    });
    row(page, form, "Signing Key", |page, r| {
        if !page.app.panels.preferences.signing_keys_read {
            refresh_signing_keys(page.app);
        }
        let key = git_value(page.app, Scope::Global, "user.signingkey");
        let keys = page.app.panels.preferences.signing_keys.clone();
        let shown = signing_key_option(&key, &keys);
        page.native_select(
            "##signing_key",
            r.x(),
            r.top,
            257.0,
            &shown,
            &keys,
            |_, app, option| set_signing_key(app, option),
        );
        r.column = 34.0;
        // The refresh button: btn-primary with a 12px refresh icon.
        let blue = page.cx.palette.accent;
        page.boxed(
            r.x() + 262.0,
            r.top,
            38.0,
            34.0,
            BoxStyle::new(blue.with_alpha(0.2), blue, 2.0),
        );
        let ink = page.ink(0.75);
        page.svg(
            "arrows-rotate",
            512.0,
            512.0,
            r.x() + 275.0,
            r.top + 11.0,
            12.0,
            12.0,
            ink,
        );
        if page.hit("##refresh_keys", r.x() + 262.0, r.top, 38.0, 34.0, true) {
            refresh_signing_keys(page.app);
        }
    });
    row(page, form, "Generate new GPG Key", |page, r| {
        let mut passphrase = std::mem::take(&mut page.app.panels.preferences.passphrase);
        let field = Field::default()
            .placeholder("Passphrase (optional)")
            .flags(InputTextFlags::PASSWORD)
            .inset();
        page.text_field(
            "##passphrase",
            r.x(),
            r.top,
            216.89,
            &mut passphrase,
            &field,
            34.0,
        );
        r.column = 34.0;
        if page.button(
            "##generate_gpg",
            r.x() + 221.89,
            r.top,
            "Generate",
            Tone::Success,
            BTN_34,
            false,
        ) {
            generate_gpg_key(page.app, &passphrase);
            passphrase.clear();
        }
        page.app.panels.preferences.passphrase = passphrase;
        help(
            page,
            r,
            "This will create a 4096 bit key with the name and email\nin your current profile that expires in 2 \
             years.\nThis will also be set as your active signing key.",
        );
    });
    row(page, form, "Sign Commits by Default", |page, r| {
        git_checkbox(
            page,
            r,
            "##sign_commits",
            Scope::Global,
            "commit.gpgsign",
            false,
            false,
        );
        help(
            page,
            r,
            "Sets commit.gpgsign: the commit form's \"Sign commit\" starts checked.",
        );
    });
    row(page, form, "Sign Tags by Default", |page, r| {
        git_checkbox(
            page,
            r,
            "##sign_tags",
            Scope::Global,
            "tag.gpgSign",
            false,
            false,
        );
        help(
            page,
            r,
            "Sets tag.gpgSign: tags ImGit creates are signed, annotated tags.",
        );
    });
}
