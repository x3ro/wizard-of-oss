use anyhow::{format_err, Context};
use slack_morphism::SlackMessageAttachmentFieldObject;
use url::Url;

#[derive(Debug, Clone)]
pub struct OpenSourceAttachment {
    pub username: String,
    pub number_of_hours: i16,
    pub country: String,
    pub url: Url,
    pub description: String,
}

impl TryFrom<Vec<SlackMessageAttachmentFieldObject>> for OpenSourceAttachment {
    type Error = anyhow::Error;

    fn try_from(fields: Vec<SlackMessageAttachmentFieldObject>) -> Result<Self, Self::Error> {
        let mut username: Result<String, anyhow::Error> = Err(format_err!("missing username"));
        let mut number_of_hours = Err(format_err!("missing number of hours"));
        let mut country = Err(format_err!("missing country"));
        let mut url = Err(format_err!("missing url"));
        let mut description = Err(format_err!("missing description"));

        for field in &fields {
            let Some(title) = &field.title else {
                continue;
            };

            let Some(value) = &field.value else {
                continue;
            };

            match title.as_str() {
                "Author" => username = Ok(value.clone()),
                "Time" => number_of_hours = value.parse::<i16>().context(value.clone()),
                "Office" => country = Ok(value.clone()),
                "URL" => {
                    url = {
                        // Slack adds pointy brackets around the URL, for formatting reasons apparently
                        let trimmed = value.trim_start_matches('<').trim_end_matches('>');
                        Url::parse(trimmed).context(value.clone())
                    }
                }
                "Description" => description = Ok(value.clone()),
                title => Err(format_err!("unknown field name '{title}'"))?,
            }
        }

        Ok(OpenSourceAttachment {
            username: username?,
            number_of_hours: number_of_hours?,
            country: country?,
            url: url?,
            description: description?,
        })
    }
}

impl From<OpenSourceAttachment> for Vec<SlackMessageAttachmentFieldObject> {
    fn from(value: OpenSourceAttachment) -> Self {
        vec![
            SlackMessageAttachmentFieldObject {
                title: Some("Author".into()),
                value: Some(value.username),
                short: Some(true),
            },
            SlackMessageAttachmentFieldObject {
                title: Some("Time".into()),
                value: Some(value.number_of_hours.to_string()),
                short: Some(true),
            },
            SlackMessageAttachmentFieldObject {
                title: Some("Office".into()),
                value: Some(value.country),
                short: Some(true),
            },
            SlackMessageAttachmentFieldObject {
                title: Some("URL".into()),
                value: Some(value.url.to_string()),
                short: Some(true),
            },
            SlackMessageAttachmentFieldObject {
                title: Some("Description".into()),
                value: Some(value.description),
                short: Some(false),
            },
        ]
    }
}
