#![cfg(windows)]

use rmux_pty::{PtyPair, TerminalSize};

#[test]
fn conpty_pair_opens_resizes_and_clones_master() -> Result<(), Box<dyn std::error::Error>> {
    let pair = PtyPair::open_with_size(TerminalSize::new(100, 30))?;
    assert_eq!(pair.master().size()?, TerminalSize::new(100, 30));

    pair.master().resize(TerminalSize::new(120, 40))?;
    assert_eq!(pair.master().size()?, TerminalSize::new(120, 40));

    let clone = pair.master().try_clone()?;
    assert_eq!(clone.size()?, TerminalSize::new(120, 40));
    Ok(())
}
