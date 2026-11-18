use super::*;
use crate::ai::claude_code::session::{self, Files, Pay};

/// A made-up picture in the test's `chat-images/`: its id.
fn picture(shade: u8) -> String {
    let png = images::encode_png(&[shade, 90, 160, 255].repeat(4 * 3), 4, 3, true).unwrap();
    images::save(&png, 4, 3).unwrap().id
}

/// A folder of the test's own.
fn scratch(tag: &str) -> PathBuf {
    let mut raw = [0u8; 6];
    let _ = getrandom::fill(&mut raw);
    let hex: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let d = std::env::temp_dir().join(format!("yap-shots-test-{tag}-{hex}")).join(FOLDER);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn names(paths: &[PathBuf]) -> Vec<String> {
    paths.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect()
}

#[test]
fn a_chats_folder_is_named_by_its_id_and_when_it_was_made() {
    assert_eq!(key(7, 1_794_793_391), "7-1794793391");
    assert_eq!(parse_key("7-1794793391"), Some((7, 1_794_793_391)));
    for not_ours in ["7", "7-", "-1794793391", "seven-1", "7-1-2", "7-1.json", "../7-1", "shot-1.png", ""] {
        assert_eq!(parse_key(not_ours), None, "{not_ours}");
    }
    assert_eq!(file_name(3), "shot-3.png");
    assert_eq!(number_of("shot-3.png"), Some(3));
    assert_eq!(number_of("shot-0.png"), None);
    assert_eq!(number_of("shot-3.tmp"), None);
}

#[test]
fn each_screenshot_is_a_file_numbered_as_it_came_and_one_sent_again_keeps_its_own() {
    let root = scratch("save");
    let (a, b, c) = (picture(10), picture(20), picture(30));
    let made = save_in(&root, "4-1700000000", &[a.clone(), b.clone()]);
    assert_eq!(names(&made), ["shot-1.png", "shot-2.png"]);
    assert!(made.iter().all(|p| p.parent() == Some(root.join("4-1700000000").as_path())));
    // The picture itself, byte for byte.
    assert_eq!(std::fs::read(&made[0]).unwrap(), images::read(&a).unwrap());
    assert_eq!(std::fs::read(&made[1]).unwrap(), images::read(&b).unwrap());
    // Which picture each is: beside the folder, never in it.
    let listed: Vec<String> = std::fs::read_dir(root.join("4-1700000000")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(listed.len(), 2, "nothing but the shots in the folder: {listed:?}");
    assert!(index_path(&root, "4-1700000000").is_file());

    // One sent again keeps its file; a new one is the next number; one
    // that's gone, or no id at all, is skipped.
    let again = save_in(&root, "4-1700000000", &[b.clone(), "img_00000000000000ff".into(), "../../secrets".into(), c.clone()]);
    assert_eq!(names(&again), ["shot-2.png", "shot-3.png"]);

    // Its file deleted (Claude may, in Bypass): copied again, same number.
    std::fs::remove_file(&made[0]).unwrap();
    assert_eq!(names(&save_in(&root, "4-1700000000", std::slice::from_ref(&a))), ["shot-1.png"]);
    assert!(made[0].is_file());

    // Another chat's folder numbers its own.
    assert_eq!(names(&save_in(&root, "5-1700000100", std::slice::from_ref(&c))), ["shot-1.png"]);
    // Nothing asked: nothing made.
    assert!(save_in(&root, "6-1700000200", &[]).is_empty());
    assert!(!root.join("6-1700000200").exists());
}

#[test]
fn an_index_that_cant_be_read_starts_again_past_every_file_there() {
    let root = scratch("unread");
    let (a, b) = (picture(40), picture(50));
    save_in(&root, "2-1700000000", &[a.clone(), b]);
    std::fs::write(index_path(&root, "2-1700000000"), "{ not json").unwrap();
    // Never written over shot-1 (another picture, as far as anyone knows now).
    let c = picture(60);
    assert_eq!(names(&save_in(&root, "2-1700000000", std::slice::from_ref(&c))), ["shot-3.png"]);
    assert_eq!(std::fs::read(root.join("2-1700000000").join("shot-1.png")).unwrap(), images::read(&a).unwrap());
}

#[test]
fn the_line_after_a_messages_words_names_its_files() {
    let one = [PathBuf::from(r"C:\Users\imogen\AppData\Roaming\yap\code-shots\3-1700000000\shot-1.png")];
    assert_eq!(paths_line(&one, true).unwrap(), r"(This screenshot is also a file you can read or pass to a helper: C:\Users\imogen\AppData\Roaming\yap\code-shots\3-1700000000\shot-1.png)");
    let two = [PathBuf::from(r"D:\yap\code-shots\3-1\shot-1.png"), PathBuf::from(r"D:\yap\code-shots\3-1\shot-2.png")];
    assert_eq!(
        paths_line(&two, true).unwrap(),
        r"(These screenshots are also files you can read or pass to a helper, in the order they came: D:\yap\code-shots\3-1\shot-1.png, D:\yap\code-shots\3-1\shot-2.png)"
    );
    // Not every picture has its file: never said to be in their order.
    assert_eq!(paths_line(&one[..], false).unwrap(), r"(Some of these screenshots are also files you can read or pass to a helper: C:\Users\imogen\AppData\Roaming\yap\code-shots\3-1700000000\shot-1.png)");
    assert_eq!(paths_line(&[], true), None);
    assert_eq!(paths_line(&[], false), None);
}

#[test]
fn a_chats_folder_reaches_its_runs_as_add_dir_each_once() {
    let mut spec = Spec::new(r"C:\Users\imogen\Projects\garden-planner", Pay::Plan);
    // (The chat's own added folder, first: kept.)
    spec.add_dirs = vec![PathBuf::from(r"D:\Shared\seeds")];
    let shots = PathBuf::from(r"C:\Users\imogen\AppData\Roaming\yap\code-shots\3-1700000000");
    apply(&mut spec, vec![shots.clone()]);
    apply(&mut spec, vec![shots.clone()]);
    assert_eq!(spec.add_dirs, [PathBuf::from(r"D:\Shared\seeds"), shots.clone()]);
    let a = session::args(&spec, &Files { settings: None, mcp_config: None, system_prompt: None, own_prompt: None });
    let at = a.iter().position(|x| x == "--add-dir").unwrap();
    assert_eq!(a[at + 1..at + 3], [r"D:\Shared\seeds".to_string(), shots.display().to_string()]);
    // No chat: no folder.
    assert!(run_dirs(None, true, None).is_empty());
}

#[test]
fn pictures_deleted_take_their_files_from_every_chat() {
    let root = scratch("forget");
    let (a, b) = (picture(70), picture(80));
    let first = save_in(&root, "1-1700000000", &[a.clone(), b.clone()]);
    let forked = save_in(&root, "2-1700000500", std::slice::from_ref(&a));
    assert_eq!(forget_in(&root, std::slice::from_ref(&a)), 2);
    assert!(!first[0].exists() && !forked[0].exists());
    assert!(first[1].is_file(), "the other picture's file stays");
    assert_eq!(read_index(&root, "1-1700000000").unwrap().shots, [Shot { n: 2, id: b.clone() }]);
    // Sent again later: a new number, never the one that went.
    assert_eq!(names(&save_in(&root, "1-1700000000", std::slice::from_ref(&a))), ["shot-3.png"]);
    assert_eq!(forget_in(&root, &[]), 0);
    assert_eq!(forget_in(&root.join("nothing-here"), std::slice::from_ref(&b)), 0);
}

#[test]
fn a_chat_deleted_takes_its_folder_whatever_chat_made_it() {
    let root = scratch("deleted");
    let a = picture(90);
    save_in(&root, "3-1700000000", std::slice::from_ref(&a));
    // (An older one of the same id, a chat deleted where its hook never ran.)
    save_in(&root, "3-1600000000", std::slice::from_ref(&a));
    save_in(&root, "33-1700000000", std::slice::from_ref(&a));
    std::fs::create_dir_all(root.join("not-ours")).unwrap();
    assert_eq!(chat_deleted_in(&root, 3), 2);
    assert!(!root.join("3-1700000000").exists() && !index_path(&root, "3-1700000000").exists());
    assert!(!root.join("3-1600000000").exists());
    assert!(root.join("33-1700000000").join("shot-1.png").is_file(), "another chat's stays");
    assert!(root.join("not-ours").is_dir(), "nothing that isn't a chat's is touched");
}

#[test]
fn the_sweep_keeps_a_file_as_long_as_its_picture_and_a_folder_as_long_as_its_chat() {
    let root = scratch("sweep");
    let (a, b) = (picture(100), picture(110));
    let kept = save_in(&root, "1-1700000000", &[a.clone(), b.clone()]);
    save_in(&root, "2-1700000000", std::slice::from_ref(&a));
    // (A copy cut short, and a file Claude left there.)
    let folder = root.join("1-1700000000");
    std::fs::write(folder.join("shot-9.tmp"), b"x").unwrap();
    std::fs::write(folder.join("notes.txt"), b"x").unwrap();
    std::fs::create_dir_all(root.join("not-ours")).unwrap();
    let now = crate::agents::store::now_secs();
    // Chat 2 is gone; picture b is past "Keep chat screenshots".
    let alive = |c: u64, created: u64| c == 1 && created == 1_700_000_000;
    let exists = |id: &str| id == a;
    // Within the hour: the stray files stay.
    assert_eq!(sweep_in(&root, &alive, &exists, now), 2, "chat 2's folder and b's file");
    assert!(!root.join("2-1700000000").exists() && !index_path(&root, "2-1700000000").exists());
    assert!(kept[0].is_file() && !kept[1].exists());
    assert_eq!(read_index(&root, "1-1700000000").unwrap().shots, [Shot { n: 1, id: a.clone() }]);
    assert!(folder.join("notes.txt").is_file() && folder.join("shot-9.tmp").is_file());
    // An hour on: they go too.
    assert_eq!(sweep_in(&root, &alive, &exists, now + STRAY_SECS + 60), 2);
    assert!(!folder.join("notes.txt").exists() && !folder.join("shot-9.tmp").exists());
    assert!(kept[0].is_file());
    // Its last picture gone: the folder and its index.
    assert_eq!(sweep_in(&root, &alive, &|_: &str| false, now), 1);
    assert!(!folder.exists() && !index_path(&root, "1-1700000000").exists());
    assert!(root.join("not-ours").is_dir());
    // No folder at all: nothing to do.
    assert_eq!(sweep_in(&root.join("nothing-here"), &alive, &exists, now), 0);
}
