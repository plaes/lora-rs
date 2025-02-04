use super::*;
use core::cmp::Ordering;

#[derive(Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct JoinChannels {
    /// The maximum amount of times we attempt to join on the preferred subband.
    max_retries: usize,
    /// The amount of times we've currently attempted to join on the preferred subband.
    pub num_retries: usize,
    /// Preferred subband
    preferred_subband: Option<Subband>,
    /// Channels that have been attempted.
    pub(crate) available_channels: AvailableChannels,
}

impl JoinChannels {
    pub(crate) fn set_join_bias(&mut self, subband: Subband, max_retries: usize) {
        self.preferred_subband = Some(subband);
        self.max_retries = max_retries;
    }

    pub(crate) fn clear_join_bias(&mut self) {
        self.preferred_subband = None;
        self.max_retries = 0;
    }

    /// To be called after a join accept is received. Resets state for the next join attempt.
    pub(crate) fn reset(&mut self) {
        self.num_retries = 0;
        self.available_channels = AvailableChannels::default();
    }

    pub(crate) fn get_next_channel(&mut self, rng: &mut impl RngCore) -> usize {
        match (self.preferred_subband, self.num_retries.cmp(&self.max_retries)) {
            (Some(sb), Ordering::Less) => {
                self.num_retries += 1;
                // pick a  random number 0-7 on the preferred subband
                // NB: we don't use 500 kHz channels
                let channel = (rng.next_u32() as usize % 8) + ((sb as usize - 1) * 8);
                if self.num_retries == self.max_retries {
                    // this is our last try with our favorite subband, so will intialize the
                    // standard join logic with the channel we just tried. This will ensure
                    // standard and compliant behavior when num_retries is set to 1.
                    self.available_channels.previous = Some(channel);
                    self.available_channels.data.set_channel(channel, false);
                }
                channel
            }
            _ => self.available_channels.get_next(rng),
        }
    }
}

#[derive(Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct AvailableChannels {
    data: ChannelMask<9>,
    previous: Option<usize>,
}

impl AvailableChannels {
    fn is_exhausted(&self) -> bool {
        // check if every underlying byte is entirely cleared to 0
        for byte in self.data.as_ref() {
            if *byte != 0 {
                return false;
            }
        }
        true
    }

    fn get_next(&mut self, rng: &mut impl RngCore) -> usize {
        // this guarantees that there will be _some_ open channel available
        if self.is_exhausted() {
            self.reset();
        }

        let channel = self.get_next_channel_inner(rng);
        // mark the channel invalid for future selection
        self.data.set_channel(channel, false);
        self.previous = Some(channel);
        channel
    }

    fn get_next_channel_inner(&mut self, rng: &mut impl RngCore) -> usize {
        if let Some(previous) = self.previous {
            // choose the next one by possibly wrapping around
            let next = (previous + 8) % 72;
            // if the channel is valid, great!
            if self.data.is_enabled(next).unwrap() {
                next
            } else {
                // We've wrapped around to our original random bank.
                // Randomly select a new channel on the original bank.
                // NB: there shall always be something because this will be the first
                // bank to get exhausted and the caller of this function will reset
                // when the last one is exhausted.
                let bank = next / 8;
                let mut entropy = rng.next_u32() as usize;
                let mut channel = (entropy & 0b111) + bank * 8;
                let mut entropy_used = 1;
                loop {
                    if self.data.is_enabled(channel).unwrap() {
                        return channel;
                    } else {
                        // we've used 30 of the 32 bits of entropy. reset the byte
                        if entropy_used == 10 {
                            entropy = rng.next_u32() as usize;
                            entropy_used = 0;
                        }
                        entropy >>= 3;
                        entropy_used += 1;
                        channel = (entropy & 0b111) + bank * 8;
                    }
                }
            }
        } else {
            // pick a completely random channel on the bottom 64
            // NB: all channels are currently valid
            (rng.next_u32() as usize) & 0b111111
        }
    }

    fn reset(&mut self) {
        self.data = ChannelMask::default();
        self.previous = None;
    }
}

/// This macro implements public functions relating to a fixed plan region. This is preferred to a
/// trait implementation because the user does not have to worry about importing the trait to make
/// use of these functions.
macro_rules! impl_join_bias {
    ($region:ident) => {
        impl $region {
            /// Create this struct directly if you want to specify a subband on which to bias the join process.
            pub fn new() -> Self {
                Self::default()
            }

            /// Specify a preferred subband when joining the network. Only the first join attempt
            /// will occur on this subband. After that, each bank will be attempted sequentially
            /// as described in the US915/AU915 regional specifications.
            pub fn set_join_bias(&mut self, subband: Subband) {
                self.0.join_channels.set_join_bias(subband, 1)
            }

            /// # ⚠️Warning⚠️
            ///
            /// This method is explicitly not compliant with the LoRaWAN spec when more than one
            /// try is attempted.
            ///
            /// This method is similar to `set_join_bias`, but allows you to specify a potentially
            /// non-compliant amount of times your preferred join subband should be attempted.
            ///
            /// It is recommended to set a low number (ie, < 10) of join retries using the
            /// preferred subband. The reason for this is if you *only* try to join
            /// with a channel bias, and the network is configured to use a
            /// strictly different set of channels than the ones you provide, the
            /// network will NEVER be joined.
            pub fn set_join_bias_and_noncompliant_retries(
                &mut self,
                subband: Subband,
                max_retries: usize,
            ) {
                self.0.join_channels.set_join_bias(subband, max_retries)
            }

            pub fn clear_join_bias(&mut self) {
                self.0.join_channels.clear_join_bias()
            }
        }
    };
}

impl_join_bias!(US915);
impl_join_bias!(AU915);

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_join_channels_standard() {
        let mut rng = rand_core::OsRng;
        // run the test a bunch of times due to the rng
        for _ in 0..100 {
            let mut join_channels = JoinChannels::default();
            let first_channel = join_channels.get_next_channel(&mut rng);
            // the first channel is always in the bottom 64
            assert!(first_channel < 64);
            let next_channel = join_channels.get_next_channel(&mut rng);
            // the next channel is always incremented by 8, since we always have
            // the fat bank (channels 64-71)
            assert_eq!(next_channel, first_channel + 8);
            // we generate 6 more channels
            for _ in 0..7 {
                let c = join_channels.get_next_channel(&mut rng);
                assert!(c < 72);
            }
            // after 8 tries, we should be back at the original bank but on a different channel
            let ninth_channel = join_channels.get_next_channel(&mut rng);
            assert_eq!(ninth_channel / 8, first_channel / 8);
            assert_ne!(ninth_channel, first_channel);
        }
    }

    #[test]
    fn test_join_channels_standard_exhausted() {
        let mut rng = rand_core::OsRng;

        let mut join_channels = JoinChannels::default();
        let first_channel = join_channels.get_next_channel(&mut rng);
        // the first channel is always in the bottom 64
        assert!(first_channel < 64);
        let next_channel = join_channels.get_next_channel(&mut rng);
        // the next channel is always incremented by 8, since we always have
        // the fat bank (channels 64-71)
        assert_eq!(next_channel, first_channel + 8);
        // we generate 6000
        for _ in 0..6000 {
            let c = join_channels.get_next_channel(&mut rng);
            assert!(c < 72);
        }
    }

    #[test]
    fn test_join_channels_biased() {
        let mut rng = rand_core::OsRng;
        // run the test a bunch of times due to the rng
        for _ in 0..100 {
            let mut join_channels = JoinChannels::default();
            join_channels.set_join_bias(Subband::_2, 1);
            let first_channel = join_channels.get_next_channel(&mut rng);
            // the first is on subband 2
            assert!(first_channel > 7);
            assert!(first_channel < 16);
            let next_channel = join_channels.get_next_channel(&mut rng);
            // the next channel is always incremented by 8, since we always have
            // the fat bank (channels 64-71)
            assert_eq!(next_channel, first_channel + 8);
            // we generate 6 more channels
            for _ in 0..7 {
                let c = join_channels.get_next_channel(&mut rng);
                assert!(c < 72);
            }
            // after 8 tries, we should be back at the biased bank but on a different channel
            let ninth_channel = join_channels.get_next_channel(&mut rng);
            assert_eq!(ninth_channel / 8, first_channel / 8);
            assert_ne!(ninth_channel, first_channel);
        }
    }
}
