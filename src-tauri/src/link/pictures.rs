//! **A question's pictures from the phone** (2026-11-03): each one a
//! `picture {n, size}` message, then its bytes in `0x03` frames
//! (`wire::PICTURE`), the next question (`ask` / `stop` with `pictures: N`)
//! taking the N that came whole. The page has made each a JPEG of at most
//! 2,048 px; here only the caps are kept — [`wire::MAX_PICTURES`] a question,
//! [`wire::MAX_PICTURE_BYTES`] each, in order, every piece in turn — and the
//! bytes handed on. They're read as pictures on the PC (`snip::attach::keep`,
//! as a picture pasted into Chat is: decoded, stored again as a PNG, so
//! nothing that isn't a picture is kept). Pure.

use crate::link::wire::{MAX_PICTURES, MAX_PICTURE_BYTES};

/// Words the phone shows when its pictures can't go.
pub const TOO_MANY: &str = "A question can carry 4 pictures at most.";
pub const TOO_BIG: &str = "That picture is too big to send from the phone.";
pub const BROKEN: &str = "A picture didn't reach your PC whole. Send the question again.";

/// One picture on its way.
#[derive(Debug)]
struct Coming {
    size: usize,
    next: u32,
    bytes: Vec<u8>,
}

/// The pictures the next question takes, as they come.
#[derive(Debug, Default)]
pub struct Pictures {
    whole: Vec<Vec<u8>>,
    coming: Option<Coming>,
    /// Something went wrong since the last question: it takes none.
    broken: Option<&'static str>,
}

impl Pictures {
    /// `picture {n, size}`: picture `n` (0 starts a fresh set) of `size`
    /// bytes comes next.
    pub fn announce(&mut self, n: u32, size: u64) -> Result<(), &'static str> {
        if n == 0 {
            *self = Pictures::default();
        }
        let fail = |me: &mut Pictures, why: &'static str| {
            me.coming = None;
            me.broken = Some(why);
            Err(why)
        };
        if n >= MAX_PICTURES {
            return fail(self, TOO_MANY);
        }
        if size == 0 || size > MAX_PICTURE_BYTES {
            return fail(self, TOO_BIG);
        }
        if self.coming.is_some() || n as usize != self.whole.len() {
            return fail(self, BROKEN);
        }
        self.coming = Some(Coming { size: size as usize, next: 0, bytes: Vec::with_capacity(size as usize) });
        Ok(())
    }

    /// A piece of the picture on its way. Done when it's whole.
    pub fn piece(&mut self, seq: u32, bytes: &[u8]) -> Result<(), &'static str> {
        let Some(c) = self.coming.as_mut() else {
            // Pieces of a picture refused already: dropped quietly.
            return if self.broken.is_some() { Ok(()) } else { Err(BROKEN) };
        };
        if seq != c.next || c.bytes.len() + bytes.len() > c.size {
            self.coming = None;
            self.broken = Some(BROKEN);
            return Err(BROKEN);
        }
        c.next += 1;
        c.bytes.extend_from_slice(bytes);
        if c.bytes.len() == c.size {
            if let Some(whole) = self.coming.take() {
                self.whole.push(whole.bytes);
            }
        }
        Ok(())
    }

    /// The question takes its `count` pictures: all of them, whole, or
    /// none (and why). Either way the next question starts afresh.
    pub fn take(&mut self, count: u32) -> Result<Vec<Vec<u8>>, &'static str> {
        let me = std::mem::take(self);
        if count == 0 {
            return Ok(Vec::new());
        }
        if let Some(why) = me.broken {
            return Err(why);
        }
        if count > MAX_PICTURES {
            return Err(TOO_MANY);
        }
        if me.coming.is_some() || me.whole.len() != count as usize {
            return Err(BROKEN);
        }
        Ok(me.whole)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn send(p: &mut Pictures, n: u32, bytes: &[u8], piece: usize) {
        p.announce(n, bytes.len() as u64).unwrap();
        for (i, chunk) in bytes.chunks(piece).enumerate() {
            p.piece(i as u32, chunk).unwrap();
        }
    }

    #[test]
    fn pictures_come_whole_in_pieces_and_a_question_takes_them() {
        let mut p = Pictures::default();
        send(&mut p, 0, &[1u8; 10], 4);
        send(&mut p, 1, &[2u8; 3], 4);
        assert_eq!(p.take(2).unwrap(), vec![vec![1u8; 10], vec![2u8; 3]]);
        assert!(p.take(2).is_err(), "taken");
        // None asked for: nothing, and nothing kept for later.
        send(&mut p, 0, &[3u8; 5], 5);
        assert_eq!(p.take(0).unwrap(), Vec::<Vec<u8>>::new());
        assert!(p.take(1).is_err(), "the set went with the question before");
    }

    #[test]
    fn the_caps_hold() {
        let mut p = Pictures::default();
        assert_eq!(p.announce(0, MAX_PICTURE_BYTES + 1), Err(TOO_BIG));
        assert_eq!(p.take(1), Err(TOO_BIG), "the question says why");
        assert_eq!(p.announce(0, 0), Err(TOO_BIG));
        let mut p = Pictures::default();
        for n in 0..MAX_PICTURES {
            send(&mut p, n, &[n as u8; 2], 2);
        }
        assert_eq!(p.announce(MAX_PICTURES, 2), Err(TOO_MANY));
        assert_eq!(p.take(MAX_PICTURES), Err(TOO_MANY));
        assert_eq!(Pictures::default().take(MAX_PICTURES + 1), Err(TOO_MANY));
    }

    #[test]
    fn a_picture_out_of_turn_or_cut_short_goes_no_further() {
        // A piece missed.
        let mut p = Pictures::default();
        p.announce(0, 8).unwrap();
        p.piece(0, &[0; 4]).unwrap();
        assert_eq!(p.piece(2, &[0; 4]), Err(BROKEN));
        assert_eq!(p.piece(3, &[0; 4]), Ok(()), "the rest dropped quietly");
        assert_eq!(p.take(1), Err(BROKEN));
        // More than it said.
        let mut p = Pictures::default();
        p.announce(0, 4).unwrap();
        assert_eq!(p.piece(0, &[0; 5]), Err(BROKEN));
        // Cut short: the question comes before the last piece.
        let mut p = Pictures::default();
        p.announce(0, 8).unwrap();
        p.piece(0, &[0; 4]).unwrap();
        assert_eq!(p.take(1), Err(BROKEN));
        // Out of order: picture 1 before 0.
        let mut p = Pictures::default();
        assert_eq!(p.announce(1, 4), Err(BROKEN));
        // A piece with nothing announced.
        assert_eq!(Pictures::default().piece(0, &[1]), Err(BROKEN));
        // A fresh set (n 0) starts over after a broken one.
        let mut p = Pictures::default();
        let _ = p.announce(1, 4);
        send(&mut p, 0, &[9; 4], 4);
        assert_eq!(p.take(1).unwrap(), vec![vec![9u8; 4]]);
        // Fewer than the question says.
        let mut p = Pictures::default();
        send(&mut p, 0, &[9; 4], 4);
        assert_eq!(p.take(2), Err(BROKEN));
    }
}
