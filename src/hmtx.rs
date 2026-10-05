//! The `hmtx` table contains the horizontal metrics for each glyph.
//! All we need to do is to rewrite the table so that it matches the
//! sequence of the new glyphs. A minor pain point is that the table
//! allows omitting the advance width for the last few glyphs if it is
//! the same. In order to keep the code simple, we do not keep this optimization
//! when rewriting the table.
//! While doing so, we also rewrite the `hhea` table, which contains
//! the number of glyphs that contain both, advance width and
//! left side bearing metrics.

// The parsing logic was taken from ttf-parser.

use super::*;
use crate::Error::OverflowError;

pub fn subset(ctx: &mut Context) -> Result<()> {
    let hmtx = ctx.expect_table(Tag::HMTX).ok_or(MalformedFont)?;

    let new_metrics = {
        let mut new_metrics = vec![];

        match &ctx.interjector {
            Interjector::Dummy(_) => extract_metrics(hmtx, &mut new_metrics, ctx)?,
            #[cfg(feature = "variable-fonts")]
            Interjector::Skrifa(_) => {
                new_metrics = ctx.custom_hmtx_data.take().ok_or(Error::SubsetError)?
            }
        }

        new_metrics
    };
    let new_metrics = match ctx.advance_quantisation {
        Some(quantise) => quantise_advances(hmtx, new_metrics, quantise, ctx)?,
        None => new_metrics,
    };

    // Find out the last index we need to include the advance width for.
    let mut last_advance_width_index =
        u16::try_from(new_metrics.len()).map_err(|_| OverflowError)? - 1;
    let last_advance_width = new_metrics[last_advance_width_index as usize].0;

    for gid in new_metrics.iter().rev().skip(1) {
        if gid.0 == last_advance_width {
            last_advance_width_index -= 1;
        } else {
            break;
        }
    }

    let mut sub_hmtx = Writer::new();

    for (index, metric) in new_metrics.iter().enumerate() {
        let index = u16::try_from(index).map_err(|_| OverflowError)?;
        if index <= last_advance_width_index {
            sub_hmtx.write::<u16>(metric.0);
        }

        sub_hmtx.write::<i16>(metric.1);
    }

    ctx.push(Tag::HMTX, sub_hmtx.finish());

    let hhea = ctx.expect_table(Tag::HHEA).ok_or(MalformedFont)?;
    let mut sub_hhea = Writer::new();
    sub_hhea.extend(hhea.get(..hhea.len() - 2).ok_or(MalformedFont)?);
    sub_hhea.write::<u16>(last_advance_width_index + 1);

    ctx.push(Tag::HHEA, sub_hhea.finish());

    Ok(())
}

/// Moves the advance of each glyph by the change that `quantise` makes to its
/// default-instance advance, so that a variation keeps its full value.
fn quantise_advances(
    #[cfg_attr(not(feature = "variable-fonts"), allow(unused))] hmtx: &[u8],
    mut metrics: Vec<(u16, i16)>,
    quantise: AdvanceQuantisation,
    ctx: &mut Context,
) -> Result<Vec<(u16, i16)>> {
    let units_per_em = {
        let head = ctx.expect_table(Tag::HEAD).ok_or(MalformedFont)?;
        let mut r = Reader::new(head);
        r.skip_bytes(18);
        r.read::<u16>().ok_or(MalformedFont)?
    };
    let moved = |advance: u16, default: u16| {
        let change = i32::from(quantise(default, units_per_em)) - i32::from(default);
        u16::try_from(i32::from(advance) + change).map_err(|_| OverflowError)
    };
    match &ctx.interjector {
        Interjector::Dummy(_) => {
            for metric in &mut metrics {
                metric.0 = moved(metric.0, metric.0)?;
            }
        }
        #[cfg(feature = "variable-fonts")]
        Interjector::Skrifa(_) => {
            let mut defaults = vec![];
            extract_metrics(hmtx, &mut defaults, ctx)?;
            for (metric, (default, _)) in metrics.iter_mut().zip(defaults) {
                metric.0 = moved(metric.0, default)?;
            }
        }
    }
    Ok(metrics)
}

fn extract_metrics(
    hmtx: &[u8],
    new_metrics: &mut Vec<(u16, i16)>,
    ctx: &mut Context,
) -> Result<()> {
    // Extract the number of horizontal metrics from the `hhea` table.
    let num_h_metrics = {
        let hhea = ctx.expect_table(Tag::HHEA).ok_or(MalformedFont)?;
        let mut r = Reader::new(hhea);
        r.skip_bytes(34);
        r.read::<u16>().ok_or(MalformedFont)?
    };

    let last_advance_width = {
        let index = 4 * num_h_metrics.checked_sub(1).ok_or(OverflowError)? as usize;
        let mut r = Reader::new(hmtx.get(index..).ok_or(MalformedFont)?);
        r.read::<u16>().ok_or(MalformedFont)?
    };

    for old_gid in ctx.mapper.remapped_gids() {
        let has_advance_width = old_gid < num_h_metrics;

        let offset = if has_advance_width {
            old_gid as usize * 4
        } else {
            let num_h_metrics = num_h_metrics as usize;
            num_h_metrics * 4 + (old_gid as usize - num_h_metrics) * 2
        };

        let mut r = Reader::new(hmtx.get(offset..).ok_or(MalformedFont)?);

        if has_advance_width {
            let adv = r.read::<u16>().ok_or(MalformedFont)?;
            let lsb = r.read::<i16>().ok_or(MalformedFont)?;
            new_metrics.push((adv, lsb));
        } else {
            new_metrics.push((last_advance_width, r.read::<i16>().ok_or(MalformedFont)?));
        }
    }

    Ok(())
}
