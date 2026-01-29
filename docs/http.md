# HTTP API

Send modem requests, SMS messages and make database access requests from an HTTP REST API.

## [OpenAPI Swagger UI](https://morgverd.github.io/sms-server/)

## Routes

| Route                       | AT Command       | Description                                                                                               |
|-----------------------------|------------------|-----------------------------------------------------------------------------------------------------------|
| `POST /sms/send`            | `AT+CMGS`        | Send message `content` with a `to` target.                                                                |
| `GET /sms/network-status`   | `AT+CREG?`       | Get information about the registration status and access technology of the serving cell.                  |
| `GET /sms/signal-strength`  | `AT+CSQ`         | Get signal strength `rssi` and `ber` values.                                                              |
| `GET /sms/network-operator` | `AT+COPS?`       | Get the network operator ID, status and name.                                                             |
| `GET /sms/service-provider` | `AT+CSPN?`       | Get the the service provider name from the SIM.                                                           |
| `GET /sms/battery-level`    | `AT+CBC`         | Get the device battery `status`, `charge` and `voltage`.                                                  |
| `GET /sms/device-info`      | -                | Get Network Status, Signal Strength, Network Operator, Service Provider and Battery Level in one request. |
| `GET /gnss/status`          | `AT+CGPSSTATUS?` | Get the GNSS fix status (unknown, notfix, fix2d, fix3d).                                                  |
| `GET /gnss/location`        | `AT+CGPSINF=2`   | Get the GNSS location (longitude, latitude, altitude, utc_time).                                          |
| `POST /db/sms`              | -                | Query messages to and from a `phone_number` with pagination.                                              |
| `POST /db/latest-numbers`   | -                | Query all latest numbers (sender or receiver) with optional pagination.                                   |
| `POST /db/delivery-reports` | -                | Query all delivery reports for a `message_id` with optional pagination.                                   |
| `GET /sys/version`          | -                | Get the current build `version` content.                                                                  |
| `GET /sys/phone-number`     | -                | Optionally access the phone number used as an identifier in HTTP config.                                  |
| `POST /sys/set-log-level`   | -                | Set the tracing level filter for stdout, useful for live debugging.                                       |

## Pagination

Response pagination enables lazy loading of large datasets by retrieving data in chunks instead of fetching entire collections at once.
This is particularly useful for datasets like user messages that can grow to hundreds of records over time.

Here is an example that gets the first 10 messages.
```json
{
    "limit": 10,
    "offset": 0,
    "reverse": false
}
```

> [!TIP]
> Each pagination field is optional. If not present, the entire dataset is returned.

| Field     | Type          | Default      | Description                                                                        |
|-----------|---------------|--------------|------------------------------------------------------------------------------------|
| `limit`   | `Option<u64>` | `None` (all) | The amount of results to include at most in the response.                          |
| `offset`  | `Option<u64>` | `None`       | Starting index for search, an offset of `5` and limit of `5` would get `5-10`.     |
| `reverse` | `bool`        | `false`      | Should the results set be reversed. `true` means ascending results (oldest first). |

## Pseudocode

Here is an example implementation, reading all messages from a phone number in chunks.

```rust

// How many messages to receive in each response.
const PAGE_SIZE: usize = 20;

let mut page_index: usize = 0;
loop {

    // Create pagination request for the next page of results.
    let body = json!({
        "phone_number": "+0123456789",
        "limit": PAGE_SIZE,
        "offset": PAGE_SIZE * page_index
    });
    page_index = page_index + 1;

    // Send a HTTP request with pagination body.
    let messages = post_request("/db/sms", &body)?;
    for msg in &messages {
        println!("Message: {:?}", msg);
    }

    // If there isn't a full set of results, it must be the final page.
    if PAGE_SIZE > messages.len() {
        break;
    }
}
```