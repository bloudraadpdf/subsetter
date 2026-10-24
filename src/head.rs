//! The `head` table mostly contains information that can be reused from the
//! old table, except for the `loca` format, which depends on the size of the
//! glyph data. The checksum will be recalculated in the very end.

use super::*;

pub fn subset(ctx: &mut Context) -> Result<()> {
    let mut head = ctx.expect_table(Tag::HEAD).ok_or(MalformedFont)?.to_vec();
    let index_to_loc = head.get_mut(50..52).ok_or(MalformedFont)?;
    index_to_loc[0] = 0;
    index_to_loc[1] = ctx.long_loca as u8;
    ctx.push(Tag::HEAD, head);
    Ok(())
}

#[cfg(feature = "variable-fonts")]
pub(crate) fn update_bounds(ctx: &mut Context) -> Result<()> {
    let Some(bounds) = ctx.glyph_bounds else {
        return Ok(());
    };
    let (_, head) = ctx
        .tables
        .iter_mut()
        .find(|(tag, _)| *tag == Tag::HEAD)
        .ok_or(MalformedFont)?;
    let target = head.to_mut().get_mut(36..44).ok_or(MalformedFont)?;
    for (value, bytes) in bounds.into_iter().zip(target.chunks_exact_mut(2)) {
        bytes.copy_from_slice(&value.to_be_bytes());
    }
    Ok(())
}
