// A local stand-in for a calendar's private iCal address (calendar.rs):
// serves an ICS feed the spec builds at test time, relative to now, under a
// secret-looking path, and records every fetch. `set(ics)` swaps the feed
// (a calendar that changed); any other path is a 404 (a link that was reset).
// Plain HTTP on 127.0.0.1, which Yap accepts for this PC's own servers only.
import crypto from 'node:crypto';
import http from 'node:http';

/** `2026-10-05T14:30:00.000Z` → `20261005T143000Z`. */
export function icsTime(date) {
  return date.toISOString().replace(/[-:]/g, '').replace(/\.\d{3}/, '');
}

/** `20261005` (UTC date). */
export function icsDate(date) {
  return date.toISOString().slice(0, 10).replace(/-/g, '');
}

/** RFC 5545 TEXT escaping. */
function text(s) {
  return String(s).replace(/\\/g, '\\\\').replace(/;/g, '\\;').replace(/,/g, '\\,').replace(/\n/g, '\\n');
}

/**
 * One VEVENT. `start`/`end` are Dates (UTC date-times) unless `allDay`;
 * `attendees` are `{ name, email, partstat?, cutype? }`.
 */
export function vevent({
  uid,
  summary,
  start,
  end,
  allDay = false,
  attendees = [],
  organizer = null,
  description = null,
  location = null,
  rrule = null,
  exdates = [],
  extra = [],
}) {
  const lines = ['BEGIN:VEVENT', `UID:${uid}`, `DTSTAMP:${icsTime(new Date())}`, `SUMMARY:${text(summary)}`];
  if (allDay) {
    lines.push(`DTSTART;VALUE=DATE:${icsDate(start)}`, `DTEND;VALUE=DATE:${icsDate(end)}`);
  } else {
    lines.push(`DTSTART:${icsTime(start)}`, `DTEND:${icsTime(end)}`);
  }
  if (rrule) lines.push(`RRULE:${rrule}`);
  for (const d of exdates) lines.push(`EXDATE:${icsTime(d)}`);
  if (organizer) lines.push(`ORGANIZER;CN=${organizer.name}:mailto:${organizer.email}`);
  for (const a of attendees) {
    const params = [`CN="${a.name}"`, `PARTSTAT=${a.partstat ?? 'ACCEPTED'}`];
    if (a.cutype) params.push(`CUTYPE=${a.cutype}`);
    lines.push(`ATTENDEE;${params.join(';')}:mailto:${a.email}`);
  }
  if (description) lines.push(`DESCRIPTION:${text(description)}`);
  if (location) lines.push(`LOCATION:${text(location)}`);
  lines.push(...extra, 'END:VEVENT');
  return lines.join('\r\n');
}

/** A whole calendar file named `name` (Google names a secret feed after its owner). */
export function calendarFile(name, events) {
  return [
    'BEGIN:VCALENDAR',
    'VERSION:2.0',
    'PRODID:-//Yap e2e//Calendar feed//EN',
    `X-WR-CALNAME:${name}`,
    ...events,
    'END:VCALENDAR',
    '',
  ].join('\r\n');
}

export async function startCalendarFeed() {
  let body = calendarFile('Calendar', []);
  const requests = [];
  const path = `/calendar/ical/private-${crypto.randomBytes(8).toString('hex')}/basic.ics`;
  const server = http.createServer((req, res) => {
    requests.push(req.url);
    if (req.url === path) {
      res.writeHead(200, { 'content-type': 'text/calendar; charset=utf-8' });
      res.end(body);
    } else {
      res.writeHead(404).end();
    }
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const base = `http://127.0.0.1:${server.address().port}`;
  return {
    /** The feed's private address (the secret). */
    url: `${base}${path}`,
    /** An address that's gone (404). */
    missing: `${base}/calendar/ical/private-reset/basic.ics`,
    set: (ics) => (body = ics),
    requests,
    close: () => new Promise((resolve) => server.close(resolve)),
  };
}
