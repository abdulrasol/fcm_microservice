// The audience of a campaign in plain Arabic.
//
// The shape comes from the backend (`campaigns.rs`): the audience is
// `{"groups": [[topic, ...], ...]}`. The function never throws: an unknown
// shape gets a safe sentence.
//
// The Arabic words live in `strings.js`, like every other text of the
// dashboard; this file only puts them together.

import strings from '../strings'

/** The Arabic name of one topic. */
export function topicText(topic) {
  if (typeof topic !== 'string' || topic.length === 0) {
    return strings.audience.unknownTopic
  }
  if (topic === 'all') {
    return strings.audience.allUsers
  }
  if (topic.startsWith('country_')) {
    return strings.audience.country.replace('{code}', topic.slice('country_'.length).toUpperCase())
  }
  if (topic.startsWith('lang_')) {
    return strings.audience.language.replace('{code}', topic.slice('lang_'.length).toUpperCase())
  }
  return strings.audience.subscribers.replace('{topic}', topic)
}

/**
 * A sentence that says who gets the campaign.
 *
 * The audience of the backend is a list of groups, and every group is a list
 * of topics that are joined with "or".
 */
export function describeAudience(campaign) {
  try {
    const audience = campaign && campaign.audience
    const groups = audience && audience.groups
    if (!Array.isArray(groups) || groups.length === 0) {
      return strings.audience.undefined
    }
    const parts = groups.map((group) => {
      if (!Array.isArray(group) || group.length === 0) {
        return strings.audience.anyUser
      }
      return group.map((topic) => topicText(topic)).join(` ${strings.audience.or} `)
    })
    return strings.audience.recipients.replace('{parts}', parts.join(strings.audience.listSeparator))
  } catch (error) {
    return strings.audience.unknown
  }
}

export default describeAudience
