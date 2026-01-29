use anyhow::{anyhow, Result};
use sms_types::gnss::{FixStatus, PositionReport};

pub fn parse_cmgs_result(response: &str) -> Result<u8> {
    let cmgs_line = response
        .lines()
        .find(|line| line.trim().starts_with("+CMGS:"))
        .ok_or(anyhow!("No CMGS response found in buffer"))?;

    cmgs_line
        .trim()
        .strip_prefix("+CMGS:")
        .ok_or(anyhow!("Malformed CMGS response"))?
        .trim()
        .parse()
        .map_err(|_| anyhow!("Invalid CMGS message reference number"))
}

pub fn parse_creg_response(response: &str) -> Result<(u8, u8)> {
    let creg_line = response
        .lines()
        .find(|line| line.trim().starts_with("+CREG:"))
        .ok_or(anyhow!("No CREG response found in buffer"))?;

    let data = creg_line
        .trim()
        .strip_prefix("+CREG:")
        .ok_or(anyhow!("Malformed CREG response"))?
        .trim();

    let mut parts = data.split(',');
    let registration: u8 = parts
        .next()
        .ok_or(anyhow!("Missing registration status"))?
        .trim()
        .parse()
        .map_err(|_| anyhow!("Invalid registration status"))?;

    let technology: u8 = parts
        .next()
        .ok_or(anyhow!("Missing technology status"))?
        .trim()
        .parse()
        .map_err(|_| anyhow!("Invalid technology status"))?;

    Ok((registration, technology))
}

pub fn parse_csq_response(response: &str) -> Result<(i32, i32)> {
    let csq_line = response
        .lines()
        .find(|line| line.trim().starts_with("+CSQ:"))
        .ok_or(anyhow!("No CSQ response found in buffer"))?;

    let data = csq_line
        .trim()
        .strip_prefix("+CSQ:")
        .ok_or(anyhow!("Malformed CSQ response"))?
        .trim();

    let mut parts = data.split(',');
    let rssi: i32 = parts
        .next()
        .ok_or(anyhow!("Missing RSSI value"))?
        .trim()
        .parse()
        .map_err(|_| anyhow!("Invalid RSSI value"))?;

    let ber: i32 = parts
        .next()
        .ok_or(anyhow!("Missing BER value"))?
        .trim()
        .parse()
        .map_err(|_| anyhow!("Invalid BER value"))?;

    Ok((rssi, ber))
}

pub fn parse_cops_response(response: &str) -> Result<(u8, u8, String)> {
    let cops_line = response
        .lines()
        .find(|line| line.trim().starts_with("+COPS:"))
        .ok_or(anyhow!("No COPS response found in buffer"))?;

    let data = cops_line
        .trim()
        .strip_prefix("+COPS:")
        .ok_or(anyhow!("Malformed COPS response"))?
        .trim();

    let mut parts = data.split(',');
    let status: u8 = parts
        .next()
        .ok_or(anyhow!("Missing operator status"))?
        .trim()
        .parse()
        .map_err(|_| anyhow!("Invalid operator status"))?;

    let format: u8 = parts
        .next()
        .ok_or(anyhow!("Missing operator format"))?
        .trim()
        .parse()
        .map_err(|_| anyhow!("Invalid operator format"))?;

    let operator = parts
        .next()
        .ok_or(anyhow!("Missing operator name"))?
        .trim()
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or(anyhow!("Operator name not properly quoted"))?
        .to_string();

    Ok((status, format, operator))
}

pub fn parse_cspn_response(response: &str) -> Result<String> {
    let cspn_line = response
        .lines()
        .find(|line| line.trim().starts_with("+CSPN:"))
        .ok_or(anyhow!("No CSPN response found in buffer"))?;

    let data = cspn_line
        .trim()
        .strip_prefix("+CSPN:")
        .ok_or(anyhow!("Malformed CSPN response"))?
        .trim();

    // Find the quoted operator name.
    let quote_start = data
        .find('"')
        .ok_or(anyhow!("Missing opening quote for operator name"))?;
    let quote_end = data
        .rfind('"')
        .ok_or(anyhow!("Missing closing quote for operator name"))?;

    if quote_start >= quote_end {
        return Err(anyhow!("Invalid quoted operator name"));
    }
    Ok(data[quote_start + 1..quote_end].to_string())
}

pub fn parse_cbc_response(response: &str) -> Result<(u8, u8, f32)> {
    let cbc_line = response
        .lines()
        .find(|line| line.trim().starts_with("+CBC:"))
        .ok_or(anyhow!("No CBC response found in buffer"))?;

    let data = cbc_line
        .trim()
        .strip_prefix("+CBC:")
        .ok_or(anyhow!("Malformed CBC response"))?
        .trim();

    let mut parts = data.split(',');
    let status: u8 = parts
        .next()
        .ok_or(anyhow!("Missing battery status"))?
        .trim()
        .parse()
        .map_err(|_| anyhow!("Invalid battery status"))?;

    let charge: u8 = parts
        .next()
        .ok_or(anyhow!("Missing battery charge"))?
        .trim()
        .parse()
        .map_err(|_| anyhow!("Invalid battery charge"))?;

    let voltage_raw: u32 = parts
        .next()
        .ok_or(anyhow!("Missing battery voltage"))?
        .trim()
        .parse()
        .map_err(|_| anyhow!("Invalid battery voltage"))?;

    let voltage: f32 = voltage_raw as f32 / 1000.0;
    Ok((status, charge, voltage))
}

pub fn parse_cgpsstatus_response(response: &str) -> Result<FixStatus> {
    let cgps_line = response
        .lines()
        .find(|line| line.trim().starts_with("+CGPSSTATUS:"))
        .ok_or(anyhow!("No CGPSSTATUS response found in buffer"))?;

    let status_str = cgps_line
        .split_once(": ")
        .map(|(_, s)| s.trim())
        .ok_or(anyhow!("Missing CGPS status"))?;

    FixStatus::try_from(status_str).map_err(|e| anyhow!("{e:?}"))
}

pub fn parse_cgnsinf_response(response: &str, unsolicited: bool) -> Result<PositionReport> {
    let header = if unsolicited { "+UGNSINF" } else { "+CGNSINF" };
    let cgnsinf_line = response
        .lines()
        .find(|line| line.trim().starts_with(header))
        .ok_or(anyhow!("No CGNSINF response found in buffer"))?;

    let data_str = cgnsinf_line
        .split_once(": ")
        .map(|(_, s)| s.trim())
        .ok_or(anyhow!("Missing CGNSINF data"))?;

    let fields: Vec<&str> = data_str.split(',').collect();
    PositionReport::try_from(fields).map_err(|e| anyhow!("{e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cmgs_result() {
        // Success cases - test exact values
        let response = "AT+CMGS=10\r\n+CMGS: 123\r\nOK\r\n";
        let result = parse_cmgs_result(response).unwrap();
        assert_eq!(result, 123, "Expected message reference 123");

        let response = "AT+CMGS=10\r\n  +CMGS:   42  \r\nOK\r\n";
        let result = parse_cmgs_result(response).unwrap();
        assert_eq!(
            result, 42,
            "Expected message reference 42 with whitespace handling"
        );

        let response = "Some other line\r\n+CMGS: 99\r\nAnother line\r\nOK\r\n";
        let result = parse_cmgs_result(response).unwrap();
        assert_eq!(
            result, 99,
            "Expected message reference 99 with multiple lines"
        );

        // Edge case: maximum u8 value
        let response = "+CMGS: 255\r\n";
        let result = parse_cmgs_result(response).unwrap();
        assert_eq!(result, 255, "Expected maximum u8 value");

        // Edge case: minimum value
        let response = "+CMGS: 0\r\n";
        let result = parse_cmgs_result(response).unwrap();
        assert_eq!(result, 0, "Expected minimum value 0");

        // Failure cases
        let response = "AT+CMGS=10\r\nOK\r\n";
        let err = parse_cmgs_result(response).unwrap_err();
        assert!(
            err.to_string().contains("No CMGS response found"),
            "Expected 'No CMGS response found' error"
        );

        let response = "+CMGS: abc\r\n";
        let err = parse_cmgs_result(response).unwrap_err();
        assert!(
            err.to_string()
                .contains("Invalid CMGS message reference number"),
            "Expected invalid reference number error"
        );

        let response = "+CMGS: 256\r\n"; // Overflow u8
        let err = parse_cmgs_result(response).unwrap_err();
        assert!(
            err.to_string()
                .contains("Invalid CMGS message reference number"),
            "Expected error for value exceeding u8 range"
        );

        let response = "+CMGS: -1\r\n"; // Negative value
        let err = parse_cmgs_result(response).unwrap_err();
        assert!(
            err.to_string()
                .contains("Invalid CMGS message reference number"),
            "Expected error for negative value"
        );

        let response = "";
        assert!(
            parse_cmgs_result(response).is_err(),
            "Expected error for empty string"
        );
    }

    #[test]
    fn test_parse_creg_response() {
        // Success cases - test both values
        let response = "+CREG: 1,7\r\nOK\r\n";
        let (reg, tech) = parse_creg_response(response).unwrap();
        assert_eq!(reg, 1, "Expected registration status 1");
        assert_eq!(tech, 7, "Expected technology status 7");

        let response = "  +CREG:  2 , 4  \r\nOK\r\n";
        let (reg, tech) = parse_creg_response(response).unwrap();
        assert_eq!(reg, 2, "Expected registration status 2 with whitespace");
        assert_eq!(tech, 4, "Expected technology status 4 with whitespace");

        // Test various valid combinations
        let response = "+CREG: 0,0\r\n";
        let (reg, tech) = parse_creg_response(response).unwrap();
        assert_eq!(reg, 0, "Expected minimum registration status");
        assert_eq!(tech, 0, "Expected minimum technology status");

        let response = "+CREG: 5,9\r\n";
        let (reg, tech) = parse_creg_response(response).unwrap();
        assert_eq!(reg, 5, "Expected registration status 5");
        assert_eq!(tech, 9, "Expected technology status 9");

        // Failure cases
        let response = "OK\r\n";
        let err = parse_creg_response(response).unwrap_err();
        assert!(
            err.to_string().contains("No CREG response found"),
            "Expected 'No CREG response found' error"
        );

        let response = "+CREG: 1\r\n";
        let err = parse_creg_response(response).unwrap_err();
        assert!(
            err.to_string().contains("Missing technology status"),
            "Expected missing technology status error"
        );

        let response = "+CREG: abc,7\r\n";
        let err = parse_creg_response(response).unwrap_err();
        assert!(
            err.to_string().contains("Invalid registration status"),
            "Expected invalid registration status error"
        );

        let response = "+CREG: 1,xyz\r\n";
        let err = parse_creg_response(response).unwrap_err();
        assert!(
            err.to_string().contains("Invalid technology status"),
            "Expected invalid technology status error"
        );

        let response = "+CREG: 1,\r\n"; // Empty technology field
        let err = parse_creg_response(response).unwrap_err();
        assert!(
            err.to_string().contains("Invalid technology status"),
            "Expected error for empty technology field"
        );
    }

    #[test]
    fn test_parse_csq_response() {
        // Success cases - test both RSSI and BER
        let response = "+CSQ: 15,99\r\nOK\r\n";
        let (rssi, ber) = parse_csq_response(response).unwrap();
        assert_eq!(rssi, 15, "Expected RSSI value 15");
        assert_eq!(ber, 99, "Expected BER value 99");

        let response = "+CSQ: -50,-10\r\nOK\r\n";
        let (rssi, ber) = parse_csq_response(response).unwrap();
        assert_eq!(rssi, -50, "Expected negative RSSI value -50");
        assert_eq!(ber, -10, "Expected negative BER value -10");

        // Test boundary values
        let response = "+CSQ: 0,0\r\n";
        let (rssi, ber) = parse_csq_response(response).unwrap();
        assert_eq!(rssi, 0, "Expected RSSI value 0");
        assert_eq!(ber, 0, "Expected BER value 0");

        let response = "+CSQ: 31,7\r\n"; // Common max values
        let (rssi, ber) = parse_csq_response(response).unwrap();
        assert_eq!(rssi, 31, "Expected RSSI value 31");
        assert_eq!(ber, 7, "Expected BER value 7");

        // Failure cases
        let response = "ERROR\r\n";
        let err = parse_csq_response(response).unwrap_err();
        assert!(
            err.to_string().contains("No CSQ response found"),
            "Expected 'No CSQ response found' error"
        );

        let response = "+CSQ: 15\r\n";
        let err = parse_csq_response(response).unwrap_err();
        assert!(
            err.to_string().contains("Missing BER value"),
            "Expected missing BER value error"
        );

        let response = "+CSQ: abc,99\r\n";
        let err = parse_csq_response(response).unwrap_err();
        assert!(
            err.to_string().contains("Invalid RSSI value"),
            "Expected invalid RSSI value error"
        );

        let response = "+CSQ: 15,xyz\r\n";
        let err = parse_csq_response(response).unwrap_err();
        assert!(
            err.to_string().contains("Invalid BER value"),
            "Expected invalid BER value error"
        );

        let response = "+CSQ: ,99\r\n"; // Empty RSSI field
        let err = parse_csq_response(response).unwrap_err();
        assert!(
            err.to_string().contains("Invalid RSSI value"),
            "Expected error for empty RSSI field"
        );

        let response = "\r\n\r\n\r\n";
        assert!(
            parse_csq_response(response).is_err(),
            "Expected error for empty lines"
        );
    }

    #[test]
    fn test_parse_cops_response() {
        // Success cases - test all three values
        let response = "+COPS: 0,2,\"Vodafone\"\r\nOK\r\n";
        let (status, format, operator) = parse_cops_response(response).unwrap();
        assert_eq!(status, 0, "Expected operator status 0");
        assert_eq!(format, 2, "Expected operator format 2");
        assert_eq!(operator, "Vodafone", "Expected operator name 'Vodafone'");

        let response = "+COPS: 1, 0, \"T-Mobile UK\"\r\nOK\r\n";
        let (status, format, operator) = parse_cops_response(response).unwrap();
        assert_eq!(status, 1, "Expected operator status 1");
        assert_eq!(format, 0, "Expected operator format 0");
        assert_eq!(
            operator, "T-Mobile UK",
            "Expected operator name 'T-Mobile UK'"
        );

        // Test with special characters in operator name
        let response = "+COPS: 2,1,\"O2-UK\"\r\n";
        let (status, format, operator) = parse_cops_response(response).unwrap();
        assert_eq!(status, 2, "Expected operator status 2");
        assert_eq!(format, 1, "Expected operator format 1");
        assert_eq!(operator, "O2-UK", "Expected operator name with hyphen");

        // Test with empty operator name (edge case)
        let response = "+COPS: 0,2,\"\"\r\n";
        let (status, format, operator) = parse_cops_response(response).unwrap();
        assert_eq!(status, 0, "Expected operator status 0");
        assert_eq!(format, 2, "Expected operator format 2");
        assert_eq!(operator, "", "Expected empty operator name");

        // Failure cases
        let response = "ERROR\r\n";
        let err = parse_cops_response(response).unwrap_err();
        assert!(
            err.to_string().contains("No COPS response found"),
            "Expected 'No COPS response found' error"
        );

        let response = "+COPS: 0,2,Vodafone\r\n";
        let err = parse_cops_response(response).unwrap_err();
        assert!(
            err.to_string()
                .contains("Operator name not properly quoted"),
            "Expected unquoted operator name error"
        );

        let response = "+COPS: 0,2\r\n";
        let err = parse_cops_response(response).unwrap_err();
        assert!(
            err.to_string().contains("Missing operator name"),
            "Expected missing operator name error"
        );

        let response = "+COPS: abc,2,\"Vodafone\"\r\n";
        let err = parse_cops_response(response).unwrap_err();
        assert!(
            err.to_string().contains("Invalid operator status"),
            "Expected invalid operator status error"
        );

        let response = "+COPS: 0,xyz,\"Vodafone\"\r\n";
        let err = parse_cops_response(response).unwrap_err();
        assert!(
            err.to_string().contains("Invalid operator format"),
            "Expected invalid operator format error"
        );

        let response = "+COPS: 0,2,\"Vodafone\r\n"; // Missing closing quote
        let err = parse_cops_response(response).unwrap_err();
        assert!(
            err.to_string()
                .contains("Operator name not properly quoted"),
            "Expected error for missing closing quote"
        );
    }

    #[test]
    fn test_parse_cspn_response() {
        // Success cases - test operator name extraction
        let response = "+CSPN: \"EE\",0\r\nOK\r\n";
        let operator = parse_cspn_response(response).unwrap();
        assert_eq!(operator, "EE", "Expected operator name 'EE'");

        let response = "+CSPN: \"Three UK\",1\r\nOK\r\n";
        let operator = parse_cspn_response(response).unwrap();
        assert_eq!(operator, "Three UK", "Expected operator name 'Three UK'");

        // Test with special characters
        let response = "+CSPN: \"AT&T\",0\r\n";
        let operator = parse_cspn_response(response).unwrap();
        assert_eq!(operator, "AT&T", "Expected operator name with ampersand");

        let response = "+CSPN: \"Operator-Name_123\",0\r\n";
        let operator = parse_cspn_response(response).unwrap();
        assert_eq!(
            operator, "Operator-Name_123",
            "Expected operator name with mixed characters"
        );

        // Edge case: empty operator name
        let response = "+CSPN: \"\",0\r\n";
        let operator = parse_cspn_response(response).unwrap();
        assert_eq!(operator, "", "Expected empty operator name");

        // Failure cases
        let response = "ERROR\r\n";
        let err = parse_cspn_response(response).unwrap_err();
        assert!(
            err.to_string().contains("No CSPN response found"),
            "Expected 'No CSPN response found' error"
        );

        let response = "+CSPN: EE,0\r\n";
        let err = parse_cspn_response(response).unwrap_err();
        assert!(
            err.to_string().contains("Missing opening quote"),
            "Expected missing opening quote error"
        );

        let response = "+CSPN: \"EE,0\r\n";
        let err = parse_cspn_response(response).unwrap_err();
        assert!(
            err.to_string().contains("Invalid quoted operator name"),
            "Expected invalid quoted operator name error (same quote found for open and close)"
        );

        let response = "+CSPN: EE\",0\r\n"; // Missing opening quote (closing quote exists)
        let err = parse_cspn_response(response).unwrap_err();
        assert!(
            err.to_string().contains("Invalid quoted operator name"),
            "Expected error for invalid quotes (closing quote before opening)"
        );
    }

    #[test]
    fn test_parse_cbc_response() {
        // Success cases - test all three values including voltage conversion
        let response = "+CBC: 0,50,3800\r\nOK\r\n";
        let (status, charge, voltage) = parse_cbc_response(response).unwrap();
        assert_eq!(status, 0, "Expected battery status 0");
        assert_eq!(charge, 50, "Expected battery charge 50%");
        assert!(
            (voltage - 3.8).abs() < f32::EPSILON,
            "Expected voltage 3.8V, got {voltage}"
        );

        let response = "+CBC: 1,100,4123\r\nOK\r\n";
        let (status, charge, voltage) = parse_cbc_response(response).unwrap();
        assert_eq!(status, 1, "Expected battery status 1");
        assert_eq!(charge, 100, "Expected battery charge 100%");
        assert!(
            (voltage - 4.123).abs() < f32::EPSILON,
            "Expected voltage 4.123V, got {voltage}"
        );

        // Test boundary values
        let response = "+CBC: 0,0,0\r\n";
        let (status, charge, voltage) = parse_cbc_response(response).unwrap();
        assert_eq!(status, 0, "Expected battery status 0");
        assert_eq!(charge, 0, "Expected battery charge 0%");
        assert!(
            (voltage - 0.0).abs() < f32::EPSILON,
            "Expected voltage 0.0V, got {voltage}"
        );

        let response = "+CBC: 2,75,4200\r\n";
        let (status, charge, voltage) = parse_cbc_response(response).unwrap();
        assert_eq!(status, 2, "Expected battery status 2");
        assert_eq!(charge, 75, "Expected battery charge 75%");
        assert!(
            (voltage - 4.2).abs() < f32::EPSILON,
            "Expected voltage 4.2V, got {voltage}"
        );

        // Test voltage conversion precision
        let response = "+CBC: 0,50,3456\r\n";
        let (_, _, voltage) = parse_cbc_response(response).unwrap();
        assert!(
            (voltage - 3.456).abs() < 0.001,
            "Expected voltage 3.456V with proper precision, got {voltage}"
        );

        // Failure cases
        let response = "ERROR\r\n";
        let err = parse_cbc_response(response).unwrap_err();
        assert!(
            err.to_string().contains("No CBC response found"),
            "Expected 'No CBC response found' error"
        );

        let response = "+CBC: 0,50\r\n";
        let err = parse_cbc_response(response).unwrap_err();
        assert!(
            err.to_string().contains("Missing battery voltage"),
            "Expected missing battery voltage error"
        );

        let response = "+CBC: abc,50,3800\r\n";
        let err = parse_cbc_response(response).unwrap_err();
        assert!(
            err.to_string().contains("Invalid battery status"),
            "Expected invalid battery status error"
        );

        let response = "+CBC: 0,xyz,3800\r\n";
        let err = parse_cbc_response(response).unwrap_err();
        assert!(
            err.to_string().contains("Invalid battery charge"),
            "Expected invalid battery charge error"
        );

        let response = "+CBC: 0,50,abc\r\n";
        let err = parse_cbc_response(response).unwrap_err();
        assert!(
            err.to_string().contains("Invalid battery voltage"),
            "Expected invalid battery voltage error"
        );

        let response = "+CBC: 0,150,3800\r\n"; // Charge > 100 (still valid u8)
        let (_, charge, _) = parse_cbc_response(response).unwrap();
        assert_eq!(charge, 150, "Parser accepts values > 100 as valid u8");
    }

    #[test]
    fn test_parse_cgpsstatus_response() {
        // Success cases - test various fix statuses
        let response = "+CGPSSTATUS: Location 3D Fix\r\nOK\r\n";
        let result = parse_cgpsstatus_response(response);
        assert!(
            result.is_ok(),
            "Expected successful parse of '3D Fix' status"
        );
        let status = result.unwrap();
        assert_eq!(
            format!("{status:?}"),
            "Fix3D",
            "Expected Location3DFix status variant"
        );

        let response = "+CGPSSTATUS: Location Not Fix\r\nOK\r\n";
        let result = parse_cgpsstatus_response(response);
        assert!(
            result.is_ok(),
            "Expected successful parse of 'Not Fix' status"
        );

        let response = "+CGPSSTATUS: Location 2D Fix\r\nOK\r\n";
        let result = parse_cgpsstatus_response(response);
        assert!(
            result.is_ok(),
            "Expected successful parse of '2D Fix' status"
        );

        // Failure cases
        let response = "ERROR\r\n";
        let err = parse_cgpsstatus_response(response).unwrap_err();
        assert!(
            err.to_string().contains("No CGPSSTATUS response found"),
            "Expected 'No CGPSSTATUS response found' error"
        );

        let response = "+CGPSSTATUS\r\n";
        let err = parse_cgpsstatus_response(response).unwrap_err();
        assert!(
            err.to_string()
                .contains("No CGPSSTATUS response found in buffer"),
            "Expected error for missing colon"
        );

        let response = "+CGPSSTATUS:\r\n";
        let err = parse_cgpsstatus_response(response).unwrap_err();
        assert!(
            err.to_string().contains("Missing CGPS status"),
            "Expected error for empty status"
        );

        let response = "+CGPSSTATUS: Invalid Status\r\n";
        let result = parse_cgpsstatus_response(response);
        assert!(result.is_err(), "Expected error for invalid status string");
    }

    #[test]
    fn test_parse_cgnsinf_response() {
        // Success - solicited response
        let response = "+CGNSINF: 1,1,20230815120000.000,51.5074,-0.1278,85.4,0.0,0.0,1,0.9,1.2,0.8,,,10,4,,,42\r\nOK\r\n";
        let result = parse_cgnsinf_response(response, false);
        assert!(
            result.is_ok(),
            "Expected successful parse of solicited CGNSINF"
        );
        let location = result.unwrap();
        assert!(
            format!("{location:?}").contains("PositionReport"),
            "Expected PositionReport object"
        );

        // Success - unsolicited response
        let response = "+UGNSINF: 1,1,20230815120000.000,51.5074,-0.1278,85.4,0.0,0.0,1,0.9,1.2,0.8,,,10,4,,,42\r\nOK\r\n";
        let result = parse_cgnsinf_response(response, true);
        assert!(
            result.is_ok(),
            "Expected successful parse of unsolicited UGNSINF"
        );

        // Test with different coordinate values
        let response = "+CGNSINF: 1,1,20231201093045.123,-33.8688,151.2093,12.5,1.5,2.3,5,1.1,0.9,1.0,,,15,8,,,55\r\nOK\r\n";
        let result = parse_cgnsinf_response(response, false);
        assert!(
            result.is_ok(),
            "Expected successful parse with different coordinates"
        );

        // Failure cases - wrong header
        let response = "+UGNSINF: data\r\nOK\r\n";
        let err = parse_cgnsinf_response(response, false).unwrap_err();
        assert!(
            err.to_string().contains("No CGNSINF response found"),
            "Expected error when looking for +CGNSINF but found +UGNSINF"
        );

        let response = "+CGNSINF: data\r\nOK\r\n";
        let err = parse_cgnsinf_response(response, true).unwrap_err();
        assert!(
            err.to_string().contains("No CGNSINF response found"),
            "Expected error when looking for +UGNSINF but found +CGNSINF"
        );

        // Missing colon and data
        let response = "+CGNSINF\r\n";
        let err = parse_cgnsinf_response(response, false).unwrap_err();
        assert!(
            err.to_string().contains("Missing CGNSINF data"),
            "Expected error for missing colon and data"
        );

        let response = "+UGNSINF\r\n";
        let err = parse_cgnsinf_response(response, true).unwrap_err();
        assert!(
            err.to_string().contains("Missing CGNSINF data"),
            "Expected error for missing colon and data in unsolicited"
        );

        // Empty data after colon
        let response = "+CGNSINF: \r\n";
        let result = parse_cgnsinf_response(response, false);
        assert!(result.is_err(), "Expected error for empty CGNSINF data");

        // Insufficient fields
        let response = "+CGNSINF: 1,1,20230815120000.000\r\n";
        let result = parse_cgnsinf_response(response, false);
        assert!(
            result.is_err(),
            "Expected error for insufficient CGNSINF fields"
        );
    }
}
