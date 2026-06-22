use embedded_hal::{
    delay::DelayNs,
    i2c::{I2c, SevenBitAddress},
};

use crate::types::{
    Measurement, PeriodicRate, RawMeasurement, Repeatability, Result, Status, map_fetch_error,
    parse_raw_measurement, parse_raw_temperature, temperature_celsius_from_raw,
    temperature_millicelsius_from_raw,
};

use super::{
    CMD_ART, CMD_BREAK, CMD_CLEAR_STATUS, CMD_FETCH_DATA, CMD_READ_STATUS, CMD_SOFT_RESET,
    COMMAND_DELAY_MS, GENERAL_CALL_ADDRESS, Sht3x, heater_command, parse_status,
};

impl<I2C> Sht3x<I2C>
where
    I2C: I2c<SevenBitAddress>,
{
    /// Triggers one measurement without clock stretching and waits for it.
    pub fn measure<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<Measurement, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_raw(delay, repeatability)
            .map(RawMeasurement::to_measurement)
    }

    /// Triggers one measurement without clock stretching and waits for it.
    pub fn measure_raw<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<RawMeasurement, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_raw_after_delay(delay, repeatability, repeatability.delay_ms())
    }

    /// Triggers one measurement and reads only the temperature word.
    ///
    /// The read transfer requests only the first three bytes
    /// `(temperature MSB, temperature LSB, CRC)`, aborting before humidity to
    /// reduce I2C bus traffic.
    pub fn measure_temperature<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<f32, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_temperature_raw(delay, repeatability)
            .map(temperature_celsius_from_raw)
    }

    /// Triggers one measurement and reads only temperature in millidegrees Celsius.
    ///
    /// This is an integer-only alternative to [`Self::measure_temperature`].
    pub fn measure_temperature_millicelsius<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<i32, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_temperature_raw(delay, repeatability)
            .map(temperature_millicelsius_from_raw)
    }

    /// Triggers one measurement and reads only the raw temperature word.
    pub fn measure_temperature_raw<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<u16, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_temperature_raw_after_delay(delay, repeatability, repeatability.delay_ms())
    }

    /// Triggers one measurement without clock stretching at VDD below 2.4 V.
    pub fn measure_low_voltage<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<Measurement, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_raw_low_voltage(delay, repeatability)
            .map(RawMeasurement::to_measurement)
    }

    /// Triggers one raw measurement without clock stretching at VDD below 2.4 V.
    pub fn measure_raw_low_voltage<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<RawMeasurement, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_raw_after_delay(delay, repeatability, repeatability.low_voltage_delay_ms())
    }

    /// Triggers one low-voltage measurement and reads only the temperature word.
    pub fn measure_temperature_raw_low_voltage<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<u16, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_temperature_raw_after_delay(
            delay,
            repeatability,
            repeatability.low_voltage_delay_ms(),
        )
    }

    /// Triggers one measurement with clock stretching enabled.
    ///
    /// The I2C implementation must support clock stretching. ESP HAL I2C does.
    pub fn measure_with_clock_stretching(
        &mut self,
        repeatability: Repeatability,
    ) -> Result<Measurement, I2C::Error> {
        self.measure_raw_with_clock_stretching(repeatability)
            .map(RawMeasurement::to_measurement)
    }

    /// Triggers one raw measurement with clock stretching enabled.
    pub fn measure_raw_with_clock_stretching(
        &mut self,
        repeatability: Repeatability,
    ) -> Result<RawMeasurement, I2C::Error> {
        self.write_command(repeatability.single_shot_command(true))?;
        self.read_raw_measurement()
    }

    /// Triggers one clock-stretched measurement and reads only the temperature word.
    pub fn measure_temperature_raw_with_clock_stretching(
        &mut self,
        repeatability: Repeatability,
    ) -> Result<u16, I2C::Error> {
        self.write_command(repeatability.single_shot_command(true))?;
        self.read_raw_temperature()
    }

    /// Starts periodic acquisition.
    ///
    /// The SHT3x-DIS needs a 1 ms gap before it can receive the next command.
    /// Use [`Self::start_periodic_and_wait`] if the driver should enforce it.
    pub fn start_periodic(
        &mut self,
        repeatability: Repeatability,
        rate: PeriodicRate,
    ) -> Result<(), I2C::Error> {
        self.write_command(rate.command(repeatability))
    }

    /// Starts periodic acquisition and waits for the required command gap.
    pub fn start_periodic_and_wait<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
        rate: PeriodicRate,
    ) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command_and_wait(rate.command(repeatability), delay)
    }

    /// Starts periodic acquisition with accelerated response time.
    ///
    /// The SHT3x-DIS needs a 1 ms gap before it can receive the next command.
    /// Use [`Self::start_art_and_wait`] if the driver should enforce it.
    pub fn start_art(&mut self) -> Result<(), I2C::Error> {
        self.write_command(CMD_ART)
    }

    /// Starts ART mode and waits for the required command gap.
    pub fn start_art_and_wait<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command_and_wait(CMD_ART, delay)
    }

    /// Fetches one data pair from periodic acquisition.
    ///
    /// If no periodic sample is ready yet, the sensor responds to the read
    /// header with NACK and this returns [`crate::Error::NotReady`]. Other I2C
    /// failures are still returned as [`crate::Error::I2c`].
    pub fn fetch(&mut self) -> Result<Measurement, I2C::Error> {
        self.fetch_raw().map(RawMeasurement::to_measurement)
    }

    /// Fetches one raw data pair from periodic acquisition.
    ///
    /// If no periodic sample is ready yet, the sensor responds to the read
    /// header with NACK and this returns [`crate::Error::NotReady`]. Other I2C
    /// failures are still returned as [`crate::Error::I2c`].
    pub fn fetch_raw(&mut self) -> Result<RawMeasurement, I2C::Error> {
        self.write_command(CMD_FETCH_DATA)?;
        self.read_raw_measurement().map_err(map_fetch_error)
    }

    /// Stops periodic acquisition.
    ///
    /// The break command returns the sensor to single-shot mode in 1 ms.
    pub fn stop_periodic<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command(CMD_BREAK)?;
        delay.delay_ms(COMMAND_DELAY_MS);
        Ok(())
    }

    /// Performs a device-specific soft reset.
    ///
    /// The SHT3x-DIS soft reset time has a 1.5 ms maximum, so this waits 2 ms.
    pub fn soft_reset<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command(CMD_SOFT_RESET)?;
        delay.delay_ms(2);
        Ok(())
    }

    /// Performs an I2C general-call reset.
    ///
    /// This can reset every compatible device on the shared bus segment that
    /// responds to the general-call reset sequence. The reset is functionally
    /// identical to the dedicated reset pin, so this waits 2 ms to cover the
    /// worst-case power-up time.
    pub fn general_call_reset<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.i2c
            .write(GENERAL_CALL_ADDRESS, &[0x06])
            .map_err(crate::Error::I2c)?;
        delay.delay_ms(2);
        Ok(())
    }

    /// Enables or disables the internal heater.
    ///
    /// The SHT3x-DIS needs a 1 ms gap before it can receive the next command.
    /// Use [`Self::set_heater_and_wait`] if the driver should enforce it.
    pub fn set_heater(&mut self, enabled: bool) -> Result<(), I2C::Error> {
        self.write_command(heater_command(enabled))
    }

    /// Enables or disables the internal heater and waits for the required command gap.
    pub fn set_heater_and_wait<D>(&mut self, delay: &mut D, enabled: bool) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command_and_wait(heater_command(enabled), delay)
    }

    /// Reads the status register.
    pub fn status(&mut self) -> Result<Status, I2C::Error> {
        let mut data = [0; 3];
        self.write_command(CMD_READ_STATUS)?;
        self.i2c
            .read(self.address, &mut data)
            .map_err(crate::Error::I2c)?;

        parse_status(data)
    }

    /// Clears status register flags.
    ///
    /// The SHT3x-DIS needs a 1 ms gap before it can receive the next command.
    /// Use [`Self::clear_status_and_wait`] if the driver should enforce it.
    pub fn clear_status(&mut self) -> Result<(), I2C::Error> {
        self.write_command(CMD_CLEAR_STATUS)
    }

    /// Clears status register flags and waits for the required command gap.
    pub fn clear_status_and_wait<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command_and_wait(CMD_CLEAR_STATUS, delay)
    }

    fn write_command(&mut self, command: u16) -> Result<(), I2C::Error> {
        self.i2c
            .write(self.address, &command.to_be_bytes())
            .map_err(crate::Error::I2c)
    }

    fn write_command_and_wait<D>(&mut self, command: u16, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command(command)?;
        delay.delay_ms(COMMAND_DELAY_MS);
        Ok(())
    }

    fn measure_raw_after_delay<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
        delay_ms: u32,
    ) -> Result<RawMeasurement, I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command(repeatability.single_shot_command(false))?;
        delay.delay_ms(delay_ms);
        self.read_raw_measurement()
    }

    fn measure_temperature_raw_after_delay<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
        delay_ms: u32,
    ) -> Result<u16, I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command(repeatability.single_shot_command(false))?;
        delay.delay_ms(delay_ms);
        self.read_raw_temperature()
    }

    fn read_raw_measurement(&mut self) -> Result<RawMeasurement, I2C::Error> {
        let mut data = [0; 6];
        self.i2c
            .read(self.address, &mut data)
            .map_err(crate::Error::I2c)?;
        parse_raw_measurement(data)
    }

    fn read_raw_temperature(&mut self) -> Result<u16, I2C::Error> {
        let mut data = [0; 3];
        self.i2c
            .read(self.address, &mut data)
            .map_err(crate::Error::I2c)?;
        parse_raw_temperature(data)
    }
}
