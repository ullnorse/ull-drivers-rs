use embedded_hal::{
    delay::DelayNs,
    i2c::{I2c, SevenBitAddress},
};

use crate::types::{
    HeaterDuration, HeaterPower, Measurement, RawMeasurement, Repeatability, Result,
    heater_command, parse_raw_measurement, parse_serial_number,
};

use super::{CMD_READ_SERIAL, CMD_SOFT_RESET, SOFT_RESET_DELAY_MS, Sht4x};

impl<I2C> Sht4x<I2C>
where
    I2C: I2c<SevenBitAddress>,
{
    /// Runs one measurement and returns `f32` units.
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

    /// Runs one measurement and returns raw ticks. Use
    /// [`RawMeasurement::to_fixed_point`] for integer-only conversion.
    pub fn measure_raw<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<RawMeasurement, I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command(repeatability.command())?;
        delay.delay_ms(repeatability.delay_ms());
        self.read_measurement()
    }

    /// Reads the 32-bit serial number (two CRC-checked words).
    pub fn serial_number(&mut self) -> Result<u32, I2C::Error> {
        self.write_command(CMD_READ_SERIAL)?;
        let mut data = [0u8; 6];
        self.i2c
            .read(self.address, &mut data)
            .map_err(crate::Error::Bus)?;
        parse_serial_number(data)
    }

    /// Soft reset, then waits the 1 ms soft reset time.
    pub fn soft_reset<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command(CMD_SOFT_RESET)?;
        delay.delay_ms(SOFT_RESET_DELAY_MS);
        Ok(())
    }

    /// Raw escape hatch: runs a heater command and returns the high-precision
    /// measurement taken just before the heater switches off.
    ///
    /// The heater is rated for a duty cycle below 5 % and sensor
    /// specifications are not valid while it runs (section 4.8, p. 11).
    pub fn measure_with_heater_raw<D>(
        &mut self,
        delay: &mut D,
        power: HeaterPower,
        duration: HeaterDuration,
    ) -> Result<RawMeasurement, I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command(heater_command(power, duration))?;
        delay.delay_ms(duration.delay_ms());
        self.read_measurement()
    }

    /// Raw escape hatch: writes one command byte.
    pub fn write_command(&mut self, command: u8) -> Result<(), I2C::Error> {
        self.i2c
            .write(self.address, &[command])
            .map_err(crate::Error::Bus)
    }

    /// Raw escape hatch: reads six bytes (two data words with CRC) without
    /// CRC checking.
    pub fn read_response_raw(&mut self) -> Result<[u8; 6], I2C::Error> {
        let mut data = [0u8; 6];
        self.i2c
            .read(self.address, &mut data)
            .map_err(crate::Error::Bus)?;
        Ok(data)
    }

    fn read_measurement(&mut self) -> Result<RawMeasurement, I2C::Error> {
        let data = self.read_response_raw()?;
        parse_raw_measurement(data)
    }
}
