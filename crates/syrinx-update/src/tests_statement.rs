//! Latest-statement tests on synthetic releases signed by keys generated in-process (P1-39a).

use std::time::{Duration, UNIX_EPOCH};

use crate::cng;
use crate::cng::test_keys::TestKeyPair;
use crate::error::{StatementProblem, UpdateError};
use crate::{
    AcceptedStatement, DetachedSignature, FileEntry, KeyId, KeySet, MAX_STATEMENT_BYTES,
    MAX_STATEMENT_LIFETIME_SECONDS, Manifest, PackagePath, PinnedKey, SECONDS_PER_DAY,
    SignatureKind, SignedRelease, Statement, StatementTerms, Timestamp, Version, sign_release,
    verify_statement,
};

const INSTALLED: Version = Version::new(1, 2, 0);
const OFFERED: Version = Version::new(1, 3, 0);

fn at(text: &str) -> Timestamp {
    Timestamp::parse(text).expect("test timestamps are canonical")
}

fn issued() -> Timestamp {
    at("2026-11-02T10:00:00Z")
}

fn later(time: Timestamp, seconds: u64) -> Timestamp {
    time.checked_add_seconds(seconds)
        .expect("test times stay in range")
}

fn earlier(time: Timestamp, seconds: u64) -> Timestamp {
    Timestamp::from_unix_seconds(time.unix_seconds() - seconds).expect("test times stay in range")
}

fn terms(serial: u64, days: u64) -> StatementTerms {
    StatementTerms {
        serial,
        issued: issued(),
        expires: later(issued(), days * SECONDS_PER_DAY),
    }
}

/// A canonical manifest for `version` listing one synthetic file.
fn manifest_bytes(version: Version, contents: &[u8]) -> Result<Vec<u8>, UpdateError> {
    let path = PackagePath::parse("syrinx.exe").map_err(|problem| UpdateError::Manifest {
        line: 0,
        problem: crate::ManifestProblem::Path(problem),
    })?;
    let entry = FileEntry::new(path, contents.len() as u64, cng::sha256(contents)?);
    Ok(Manifest::new(version, Version::new(1, 0, 0), vec![entry])?.to_bytes())
}

/// Two pinned keys, as #55 arranges: a primary and an offline backup.
struct Keys {
    primary: TestKeyPair,
    backup: TestKeyPair,
    pinned: KeySet,
}

fn keys() -> Result<Keys, UpdateError> {
    let primary = TestKeyPair::generate()?;
    let backup = TestKeyPair::generate()?;
    let pinned = KeySet::new(&[
        PinnedKey::new(primary.public_point()?),
        PinnedKey::new(backup.public_point()?),
    ])?;
    Ok(Keys {
        primary,
        backup,
        pinned,
    })
}

/// A release signed by the primary key, plus its manifest bytes.
fn release(keys: &Keys) -> Result<(Vec<u8>, SignedRelease), UpdateError> {
    let manifest = manifest_bytes(OFFERED, b"synthetic executable")?;
    let signed = sign_release(&manifest, &keys.primary, &keys.pinned, terms(12, 45))?;
    Ok((manifest, signed))
}

/// Sign arbitrary bytes as a statement, bypassing the release tool's own checks.
fn sign_statement(signer: &TestKeyPair, bytes: &[u8]) -> Result<Vec<u8>, UpdateError> {
    let id = KeyId::of(&signer.public_point()?)?;
    let value = signer.sign_message(bytes)?;
    Ok(DetachedSignature::new(SignatureKind::Statement, id, value)
        .to_bytes()
        .to_vec())
}

fn statement_problem(result: Result<impl Sized, UpdateError>) -> Option<StatementProblem> {
    match result {
        Err(UpdateError::Statement { problem, .. }) => Some(problem),
        _ => None,
    }
}

/// The statement's text with one line's value replaced.
fn with_field(statement: &[u8], prefix: &str, value: &str) -> Vec<u8> {
    let text = String::from_utf8(statement.to_vec()).expect("statements are UTF-8");
    text.split_inclusive('\n')
        .map(|line| {
            if line.starts_with(prefix) {
                format!("{prefix}{value}\n")
            } else {
                line.to_owned()
            }
        })
        .collect::<String>()
        .into_bytes()
}

#[test]
fn a_signed_release_round_trips() -> Result<(), UpdateError> {
    let keys = keys()?;
    let (manifest, signed) = release(&keys)?;
    let verified = verify_statement(
        &signed.statement,
        &signed.statement_signature,
        &keys.pinned,
        issued(),
        None,
    )?;
    let statement = verified.statement();
    assert_eq!(statement.serial(), 12);
    assert_eq!(statement.version(), OFFERED);
    assert_eq!(*statement.manifest_sha256(), cng::sha256(&manifest)?);
    assert_eq!(statement.issued(), issued());
    assert_eq!(statement.expires(), at("2026-12-17T10:00:00Z"));
    assert_eq!(verified.key_id(), signed.key_id);
    assert_eq!(verified.accepted().serial(), 12);
    assert_eq!(
        *verified.accepted().sha256(),
        cng::sha256(&signed.statement)?
    );

    // The manifest it names verifies through it, with P1-38's checks.
    let package = verified.verify_manifest(
        &manifest,
        &signed.manifest_signature,
        &keys.pinned,
        INSTALLED,
    )?;
    assert_eq!(package.manifest().version(), OFFERED);
    Ok(())
}

#[test]
fn the_statement_has_the_documented_shape() -> Result<(), UpdateError> {
    let hash = [0x9a; 32];
    let statement = Statement::new(
        12,
        Version::new(1, 4, 0),
        hash,
        issued(),
        at("2027-01-01T10:00:00Z"),
    )?;
    let expected = format!(
        "syrinx-update-latest 1\nserial 12\nversion 1.4.0\nmanifest-sha256 {}\n\
         issued 2026-11-02T10:00:00Z\nexpires 2027-01-01T10:00:00Z\n",
        "9a".repeat(32)
    );
    assert_eq!(statement.to_bytes(), expected.as_bytes());
    assert_eq!(Statement::parse(expected.as_bytes())?, statement);
    Ok(())
}

#[test]
fn every_flipped_statement_bit_is_refused() -> Result<(), UpdateError> {
    let keys = keys()?;
    let (_, signed) = release(&keys)?;
    for index in 0..signed.statement.len() {
        for bit in 0..8 {
            let mut statement = signed.statement.clone();
            statement[index] ^= 1 << bit;
            let result = verify_statement(
                &statement,
                &signed.statement_signature,
                &keys.pinned,
                issued(),
                None,
            );
            assert!(result.is_err(), "flipped bit {bit} of byte {index} passed");
        }
    }
    Ok(())
}

#[test]
fn every_field_changed_without_resigning_is_refused() -> Result<(), UpdateError> {
    let keys = keys()?;
    let (_, signed) = release(&keys)?;
    let changes = [
        ("serial ", "13".to_owned()),
        ("version ", "1.4.0".to_owned()),
        ("manifest-sha256 ", "0".repeat(64)),
        ("issued ", "2026-11-02T10:00:01Z".to_owned()),
        ("expires ", "2026-12-17T09:59:59Z".to_owned()),
    ];
    for (prefix, value) in changes {
        let statement = with_field(&signed.statement, prefix, &value);
        assert_ne!(statement, signed.statement, "{prefix} was not changed");
        // Each change is itself a valid statement; only the signature catches it.
        Statement::parse(&statement)?;
        let result = verify_statement(
            &statement,
            &signed.statement_signature,
            &keys.pinned,
            issued(),
            None,
        );
        assert_eq!(result.err(), Some(UpdateError::BadSignature), "{prefix}");
    }
    Ok(())
}

#[test]
fn every_flipped_signature_byte_is_refused() -> Result<(), UpdateError> {
    let keys = keys()?;
    let (_, signed) = release(&keys)?;
    for index in 0..signed.statement_signature.len() {
        let mut signature = signed.statement_signature;
        signature[index] ^= 0x01;
        let result = verify_statement(&signed.statement, &signature, &keys.pinned, issued(), None);
        let expected = match index {
            0..4 => UpdateError::SignatureFormat,
            4..12 => UpdateError::UnknownKey,
            _ => UpdateError::BadSignature,
        };
        assert_eq!(result.err(), Some(expected), "byte {index}");
    }
    for length in [0, 75, 77] {
        let result = verify_statement(
            &signed.statement,
            &vec![0; length],
            &keys.pinned,
            issued(),
            None,
        );
        assert_eq!(
            result.err(),
            Some(UpdateError::SignatureLength { actual: length })
        );
    }
    Ok(())
}

#[test]
fn manifest_and_statement_signatures_cannot_stand_in_for_each_other() -> Result<(), UpdateError> {
    let keys = keys()?;
    let (manifest, signed) = release(&keys)?;

    // Each file's magic names what it signs.
    let as_statement = verify_statement(
        &signed.statement,
        &signed.manifest_signature,
        &keys.pinned,
        issued(),
        None,
    );
    assert_eq!(as_statement.err(), Some(UpdateError::SignatureFormat));
    let as_manifest = crate::verify_manifest(
        &manifest,
        &signed.statement_signature,
        &keys.pinned,
        INSTALLED,
    );
    assert_eq!(as_manifest.err(), Some(UpdateError::SignatureFormat));

    // Relabelled, a genuine manifest signature still fails on the statement header.
    let mut relabelled = signed.manifest_signature;
    relabelled[..4].copy_from_slice(b"ALS1");
    let result = verify_statement(&manifest, &relabelled, &keys.pinned, issued(), None);
    assert_eq!(statement_problem(result), Some(StatementProblem::Header));
    let mut relabelled = signed.statement_signature;
    relabelled[..4].copy_from_slice(b"AUS1");
    let result = crate::verify_manifest(&signed.statement, &relabelled, &keys.pinned, INSTALLED);
    assert!(matches!(
        result,
        Err(UpdateError::Manifest {
            problem: crate::ManifestProblem::Header,
            ..
        })
    ));
    Ok(())
}

#[test]
fn another_format_or_kind_of_statement_is_refused() -> Result<(), UpdateError> {
    let keys = keys()?;
    let (_, signed) = release(&keys)?;
    let text = String::from_utf8(signed.statement.clone()).expect("statements are UTF-8");
    for header in [
        "syrinx-update-latest 2",
        "syrinx-update-latest 0",
        "syrinx-update-manifest 1",
        "atlas-update-latest 1",
    ] {
        let changed = text.replacen("syrinx-update-latest 1", header, 1);
        let signature = sign_statement(&keys.primary, changed.as_bytes())?;
        let result = verify_statement(changed.as_bytes(), &signature, &keys.pinned, issued(), None);
        assert_eq!(statement_problem(result), Some(StatementProblem::Header));
    }
    Ok(())
}

#[test]
fn expiry_is_exclusive_and_early_issue_has_a_day_of_slack() -> Result<(), UpdateError> {
    let keys = keys()?;
    let (_, signed) = release(&keys)?;
    let expires = at("2026-12-17T10:00:00Z");
    let check = |now: Timestamp| {
        verify_statement(
            &signed.statement,
            &signed.statement_signature,
            &keys.pinned,
            now,
            None,
        )
        .map(drop)
    };

    check(earlier(expires, 1))?;
    for now in [
        expires,
        later(expires, 1),
        later(expires, 400 * SECONDS_PER_DAY),
    ] {
        assert_eq!(
            check(now).err(),
            Some(UpdateError::StatementExpired { expires, now })
        );
    }

    // A clock up to one day behind the owner's still accepts a fresh statement.
    check(earlier(issued(), SECONDS_PER_DAY))?;
    let behind = earlier(issued(), SECONDS_PER_DAY + 1);
    assert_eq!(
        check(behind).err(),
        Some(UpdateError::StatementIssuedAhead {
            issued: issued(),
            now: behind
        })
    );
    let epoch = Timestamp::from_unix_seconds(0).expect("the epoch is spellable");
    assert!(matches!(
        check(epoch),
        Err(UpdateError::StatementIssuedAhead { .. })
    ));
    Ok(())
}

#[test]
fn a_statement_lives_at_most_sixty_days() -> Result<(), UpdateError> {
    let lifetime = |seconds: u64| {
        Statement::new(1, OFFERED, [0; 32], issued(), later(issued(), seconds)).map(drop)
    };
    assert_eq!(MAX_STATEMENT_LIFETIME_SECONDS, 60 * SECONDS_PER_DAY);
    lifetime(1)?;
    lifetime(MAX_STATEMENT_LIFETIME_SECONDS)?;
    assert_eq!(
        statement_problem(lifetime(MAX_STATEMENT_LIFETIME_SECONDS + 1)),
        Some(StatementProblem::Lifetime)
    );
    assert_eq!(
        statement_problem(lifetime(0)),
        Some(StatementProblem::Lifetime)
    );
    let backwards = Statement::new(1, OFFERED, [0; 32], issued(), earlier(issued(), 1));
    assert_eq!(
        statement_problem(backwards),
        Some(StatementProblem::Lifetime)
    );

    // The release tool refuses to sign a longer statement, and a hand-signed one is refused.
    let keys = keys()?;
    let manifest = manifest_bytes(OFFERED, b"synthetic executable")?;
    let result = sign_release(&manifest, &keys.primary, &keys.pinned, terms(1, 61));
    assert_eq!(statement_problem(result), Some(StatementProblem::Lifetime));
    let (_, signed) = release(&keys)?;
    let long = with_field(&signed.statement, "expires ", "2027-01-01T10:00:01Z");
    let signature = sign_statement(&keys.primary, &long)?;
    let result = verify_statement(&long, &signature, &keys.pinned, issued(), None);
    assert_eq!(statement_problem(result), Some(StatementProblem::Lifetime));
    Ok(())
}

#[test]
fn the_backup_key_is_accepted_and_an_unpinned_key_is_not() -> Result<(), UpdateError> {
    let keys = keys()?;
    let manifest = manifest_bytes(OFFERED, b"synthetic executable")?;
    let by_backup = sign_release(&manifest, &keys.backup, &keys.pinned, terms(3, 30))?;
    let verified = verify_statement(
        &by_backup.statement,
        &by_backup.statement_signature,
        &keys.pinned,
        issued(),
        None,
    )?;
    assert_eq!(verified.key_id(), KeyId::of(&keys.backup.public_point()?)?);
    verified.verify_manifest(
        &manifest,
        &by_backup.manifest_signature,
        &keys.pinned,
        INSTALLED,
    )?;

    // The backup's signature relabelled as the primary's fails.
    let primary_id = KeyId::of(&keys.primary.public_point()?)?;
    let value = keys.backup.sign_message(&by_backup.statement)?;
    let forged = DetachedSignature::new(SignatureKind::Statement, primary_id, value).to_bytes();
    let result = verify_statement(&by_backup.statement, &forged, &keys.pinned, issued(), None);
    assert_eq!(result.err(), Some(UpdateError::BadSignature));

    // A third key signs nothing Syrinx trusts, and the release tool will not use it.
    let stranger = TestKeyPair::generate()?;
    let signature = sign_statement(&stranger, &by_backup.statement)?;
    let result = verify_statement(
        &by_backup.statement,
        &signature,
        &keys.pinned,
        issued(),
        None,
    );
    assert_eq!(result.err(), Some(UpdateError::UnknownKey));
    let result = sign_release(&manifest, &stranger, &keys.pinned, terms(4, 30));
    assert_eq!(result.err(), Some(UpdateError::UnknownKey));

    // Until #55 pins the owner's keys, every statement is refused.
    let result = verify_statement(
        &by_backup.statement,
        &by_backup.statement_signature,
        &KeySet::production()?,
        issued(),
        None,
    );
    assert_eq!(result.err(), Some(UpdateError::UnknownKey));
    Ok(())
}

#[test]
fn replays_and_reused_serials_are_refused() -> Result<(), UpdateError> {
    let keys = keys()?;
    let (_, signed) = release(&keys)?;
    let same = AcceptedStatement::new(12, cng::sha256(&signed.statement)?);
    let check = |previous: AcceptedStatement| {
        verify_statement(
            &signed.statement,
            &signed.statement_signature,
            &keys.pinned,
            issued(),
            Some(&previous),
        )
        .map(drop)
    };

    check(AcceptedStatement::new(11, [0; 32]))?;
    check(AcceptedStatement::new(0, [0; 32]))?;
    check(same)?;
    assert_eq!(
        check(AcceptedStatement::new(13, [0; 32])).err(),
        Some(UpdateError::StatementReplayed {
            accepted: 13,
            offered: 12
        })
    );
    assert_eq!(
        check(AcceptedStatement::new(u64::MAX, [0; 32])).err(),
        Some(UpdateError::StatementReplayed {
            accepted: u64::MAX,
            offered: 12
        })
    );
    assert_eq!(
        check(AcceptedStatement::new(12, [1; 32])).err(),
        Some(UpdateError::StatementSerialReused { serial: 12 })
    );
    Ok(())
}

#[test]
fn the_manifest_must_be_the_one_stated() -> Result<(), UpdateError> {
    let keys = keys()?;
    let (manifest, signed) = release(&keys)?;
    let verified = verify_statement(
        &signed.statement,
        &signed.statement_signature,
        &keys.pinned,
        issued(),
        None,
    )?;

    // Another genuine manifest for the same version is not the stated one.
    let other = manifest_bytes(OFFERED, b"other synthetic executable")?;
    let other_signed = sign_release(&other, &keys.primary, &keys.pinned, terms(13, 45))?;
    let result = verified.verify_manifest(
        &other,
        &other_signed.manifest_signature,
        &keys.pinned,
        INSTALLED,
    );
    assert_eq!(result.err(), Some(UpdateError::ManifestNotStated));

    // The stated manifest with a bad signature still fails P1-38's check.
    let mut bad = signed.manifest_signature;
    bad[40] ^= 0x01;
    let result = verified.verify_manifest(&manifest, &bad, &keys.pinned, INSTALLED);
    assert_eq!(result.err(), Some(UpdateError::BadSignature));

    // Rollback protection still applies to the version the statement names.
    let result =
        verified.verify_manifest(&manifest, &signed.manifest_signature, &keys.pinned, OFFERED);
    assert_eq!(
        result.err(),
        Some(UpdateError::NotNewer {
            installed: OFFERED,
            offered: OFFERED
        })
    );
    Ok(())
}

#[test]
fn a_statement_for_another_version_is_refused() -> Result<(), UpdateError> {
    let keys = keys()?;
    let (manifest, signed) = release(&keys)?;
    let stated = Version::new(1, 4, 0);
    let statement = Statement::new(
        14,
        stated,
        cng::sha256(&manifest)?,
        issued(),
        later(issued(), SECONDS_PER_DAY),
    )?
    .to_bytes();
    let signature = sign_statement(&keys.primary, &statement)?;
    let verified = verify_statement(&statement, &signature, &keys.pinned, issued(), None)?;
    let result = verified.verify_manifest(
        &manifest,
        &signed.manifest_signature,
        &keys.pinned,
        INSTALLED,
    );
    assert_eq!(
        result.err(),
        Some(UpdateError::StatementVersionMismatch {
            stated,
            manifest: OFFERED
        })
    );
    Ok(())
}

#[test]
fn malformed_statements_are_refused() -> Result<(), UpdateError> {
    let keys = keys()?;
    let (_, signed) = release(&keys)?;
    let text = String::from_utf8(signed.statement.clone()).expect("statements are UTF-8");
    let cases: Vec<(String, StatementProblem)> = vec![
        (
            text.trim_end().to_owned(),
            StatementProblem::MissingFinalNewline,
        ),
        (text.replace('\n', "\r\n"), StatementProblem::Encoding),
        (format!("\u{feff}{text}"), StatementProblem::Header),
        (format!("{text}\n"), StatementProblem::ExtraLine),
        (format!("{text}note 1\n"), StatementProblem::ExtraLine),
        (
            text.replace("serial 12\n", ""),
            StatementProblem::Field("serial"),
        ),
        (
            text.replace("serial 12\nversion 1.3.0\n", "version 1.3.0\nserial 12\n"),
            StatementProblem::Field("serial"),
        ),
        (
            text.replace("serial 12", "serial  12"),
            StatementProblem::Serial,
        ),
        (
            text.replace("serial 12", "serial 012"),
            StatementProblem::Serial,
        ),
        (
            text.replace("serial 12", "serial -1"),
            StatementProblem::Serial,
        ),
        (
            text.replace("serial 12", "serial 18446744073709551616"),
            StatementProblem::Serial,
        ),
        (
            text.replace("serial 12", "serial 12 "),
            StatementProblem::Serial,
        ),
        (
            text.replace("version 1.3.0", "version 1.3"),
            StatementProblem::Version,
        ),
        (
            text.replace("version 1.3.0", "version v1.3.0"),
            StatementProblem::Version,
        ),
        (
            text.replace("issued 2026", "issued  2026"),
            StatementProblem::Timestamp,
        ),
        (
            text.replace("10:00:00Z\nexpires", "10:00:00z\nexpires"),
            StatementProblem::Timestamp,
        ),
        (
            text.replace("Z\nexpires", ".000Z\nexpires"),
            StatementProblem::Timestamp,
        ),
        (
            text.replace("Z\nexpires", "+00:00\nexpires"),
            StatementProblem::Timestamp,
        ),
    ];
    for (statement, expected) in cases {
        let signature = sign_statement(&keys.primary, statement.as_bytes())?;
        let result = verify_statement(
            statement.as_bytes(),
            &signature,
            &keys.pinned,
            issued(),
            None,
        );
        assert_eq!(statement_problem(result), Some(expected), "{statement:?}");
    }

    // The hash must be lowercase, and the largest serial is still accepted.
    let hash = cng::sha256(&signed.statement)?;
    let mut upper = String::new();
    crate::manifest::push_hex(&mut upper, &hash);
    let uppercase = with_field(&signed.statement, "manifest-sha256 ", &upper.to_uppercase());
    assert_eq!(
        statement_problem(Statement::parse(&uppercase)),
        Some(StatementProblem::Hash)
    );
    let largest = with_field(&signed.statement, "serial ", &u64::MAX.to_string());
    assert_eq!(Statement::parse(&largest)?.serial(), u64::MAX);
    Ok(())
}

#[test]
fn oversized_statements_are_refused_before_their_signature() -> Result<(), UpdateError> {
    let keys = keys()?;
    let (_, signed) = release(&keys)?;
    let oversized = vec![b'a'; MAX_STATEMENT_BYTES + 1];
    let too_large = Some(UpdateError::TooLarge {
        what: "latest statement",
        limit: MAX_STATEMENT_BYTES as u64,
    });
    let result = verify_statement(&oversized, &[0; 3], &keys.pinned, issued(), None);
    assert_eq!(result.err(), too_large);
    assert_eq!(Statement::parse(&oversized).err(), too_large);
    let result = verify_statement(
        &oversized,
        &signed.statement_signature,
        &keys.pinned,
        issued(),
        None,
    );
    assert_eq!(result.err(), too_large);
    Ok(())
}

#[test]
fn the_release_tool_refuses_a_malformed_manifest() -> Result<(), UpdateError> {
    let keys = keys()?;
    let mut manifest = manifest_bytes(OFFERED, b"synthetic executable")?;
    manifest.pop();
    let result = sign_release(&manifest, &keys.primary, &keys.pinned, terms(1, 10));
    assert!(matches!(result, Err(UpdateError::Manifest { .. })));
    Ok(())
}

#[test]
fn timestamps_have_one_spelling_and_real_dates() {
    let round_trips = [
        "1970-01-01T00:00:00Z",
        "2000-02-29T12:30:45Z",
        "2024-02-29T23:59:59Z",
        "2026-11-02T10:00:00Z",
        "2100-03-01T00:00:00Z",
        "9999-12-31T23:59:59Z",
    ];
    for text in round_trips {
        let parsed = Timestamp::parse(text).expect(text);
        assert_eq!(parsed.to_string(), text);
    }
    assert_eq!(at("1970-01-01T00:00:00Z").unix_seconds(), 0);
    assert_eq!(at("2000-01-01T00:00:00Z").unix_seconds(), 946_684_800);
    assert_eq!(at("9999-12-31T23:59:59Z").unix_seconds(), 253_402_300_799);

    let refused = [
        "",
        "1969-12-31T23:59:59Z",
        "2026-02-29T00:00:00Z",
        "2100-02-29T00:00:00Z",
        "2026-04-31T00:00:00Z",
        "2026-00-10T00:00:00Z",
        "2026-13-10T00:00:00Z",
        "2026-01-00T00:00:00Z",
        "2026-01-01T24:00:00Z",
        "2026-01-01T23:60:00Z",
        "2026-12-31T23:59:60Z",
        "2026-1-01T00:00:00Z",
        "2026-01-01 00:00:00Z",
        "2026-01-01T00:00:00",
        "2026-01-01T00:00:00z",
        "+026-01-01T00:00:00Z",
        "2026-01-01T00:00:00.5Z",
        "１９７０-01-01T00:00:00Z",
    ];
    for text in refused {
        assert_eq!(Timestamp::parse(text), None, "{text:?}");
    }
}

#[test]
fn clock_edges_are_refused_rather_than_guessed() {
    let before_epoch = UNIX_EPOCH - Duration::from_secs(1);
    assert_eq!(
        Timestamp::from_system_time(before_epoch),
        Err(UpdateError::Clock)
    );
    let past_9999 = UNIX_EPOCH + Duration::from_secs(253_402_300_800);
    assert_eq!(
        Timestamp::from_system_time(past_9999),
        Err(UpdateError::Clock)
    );
    let fraction = UNIX_EPOCH + Duration::from_millis(1_999);
    assert_eq!(
        Timestamp::from_system_time(fraction).map(Timestamp::unix_seconds),
        Ok(1)
    );
    let last = at("9999-12-31T23:59:59Z");
    assert_eq!(last.checked_add_seconds(1), None);
    assert_eq!(last.checked_add_seconds(u64::MAX), None);
    assert_eq!(Timestamp::from_unix_seconds(253_402_300_800), None);
    assert!(Timestamp::now().is_ok());
}
