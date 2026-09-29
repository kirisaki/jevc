//! Upstream wire types are private and intentionally separate from the CLI protocol.
use crate::{
    error::AppError,
    model::{Context, JevRequest, Question},
    protocol::{Answer, Usage},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize)]
pub struct Request<'a> {
    state: &'a Context,
    model: &'a str,
    questions: BTreeMap<&'a str, WireQuestion<'a>>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum WireQuestion<'a> {
    Noul {
        instructions: &'a crate::model::Entry,
        #[serde(skip_serializing_if = "Option::is_none")]
        criteria: &'a Option<crate::model::NoulCriteria>,
    },
    Choice {
        instructions: &'a crate::model::Entry,
        criteria: &'a BTreeMap<String, crate::model::Entry>,
    },
    Score {
        instructions: &'a crate::model::Entry,
        criteria: &'a Vec<crate::model::Entry>,
    },
}

impl<'a> From<&'a JevRequest> for Request<'a> {
    fn from(request: &'a JevRequest) -> Self {
        Self {
            state: &request.state,
            model: &request.model,
            questions: request
                .questions
                .iter()
                .map(|(key, q)| {
                    let question = match q {
                        Question::Noul {
                            instructions,
                            criteria,
                        } => WireQuestion::Noul {
                            instructions,
                            criteria,
                        },
                        Question::Choice {
                            instructions,
                            criteria,
                        } => WireQuestion::Choice {
                            instructions,
                            criteria,
                        },
                        Question::Score {
                            instructions,
                            criteria,
                        } => WireQuestion::Score {
                            instructions,
                            criteria,
                        },
                    };
                    (key.as_str(), question)
                })
                .collect(),
        }
    }
}

#[derive(Deserialize)]
pub struct Response {
    pub model: String,
    pub answers: BTreeMap<String, WireAnswer>,
    pub usage: WireUsage,
}

#[derive(Deserialize)]
pub struct WireUsage {
    input_tokens: u64,
    output_tokens: u64,
}

impl From<WireUsage> for Usage {
    fn from(value: WireUsage) -> Self {
        Self {
            input_tokens: value.input_tokens,
            output_tokens: value.output_tokens,
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum WireAnswer {
    Noul {
        noul: f64,
    },
    Choice {
        choice: String,
        confidence: f64,
        probabilities: BTreeMap<String, f64>,
    },
    Score {
        score: f64,
        confidence: f64,
        probabilities: BTreeMap<String, f64>,
        legend: BTreeMap<String, String>,
    },
}

fn probability(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn distribution(values: &BTreeMap<String, f64>) -> bool {
    !values.is_empty()
        && values.values().copied().all(probability)
        && (values.values().sum::<f64>() - 1.0).abs() <= 0.001
}

impl WireAnswer {
    pub fn normalize(self, question: &Question) -> Result<Answer, AppError> {
        match (self, question) {
            (Self::Noul { noul }, Question::Noul { .. }) if probability(noul) => {
                Ok(Answer::Noul { value: noul })
            }
            (
                Self::Choice {
                    choice,
                    confidence,
                    probabilities,
                },
                Question::Choice { criteria, .. },
            ) if criteria.contains_key(&choice)
                && probability(confidence)
                && distribution(&probabilities)
                && criteria.keys().eq(probabilities.keys()) =>
            {
                Ok(Answer::Choice {
                    value: choice,
                    confidence,
                    probabilities,
                })
            }
            (
                Self::Score {
                    score,
                    confidence,
                    probabilities,
                    legend,
                },
                Question::Score { criteria, .. },
            ) if score.is_finite()
                && (0.0..=(criteria.len() - 1) as f64).contains(&score)
                && probability(confidence)
                && distribution(&probabilities)
                && probabilities.len() == criteria.len()
                && legend.keys().eq(probabilities.keys())
                && (0..criteria.len()).all(|i| probabilities.contains_key(&i.to_string())) =>
            {
                Ok(Answer::Score {
                    value: score,
                    confidence,
                    probabilities,
                    legend,
                })
            }
            _ => Err(AppError::invalid_response()),
        }
    }
}
