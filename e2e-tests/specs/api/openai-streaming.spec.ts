import { test, expect } from '@playwright/test';

test.describe('OpenAI chat completions (streaming)', () => {
  test.skip(!process.env.API_KEY, 'Requires API_KEY environment variable');

  test('POST /v1/chat/completions with stream:true returns SSE', async ({ request }) => {
    const response = await request.post('/v1/chat/completions', {
      data: {
        model: 'claude-sonnet-4-20250514',
        messages: [{ role: 'user', content: 'Say "hello" and nothing else.' }],
        max_tokens: 32,
        stream: true,
      },
    });
    expect(response.status()).toBe(200);

    const contentType = response.headers()['content-type'] || '';
    expect(contentType).toContain('text/event-stream');

    const text = await response.text();
    const lines = text.split('\n').filter((l) => l.startsWith('data: '));
    expect(lines.length).toBeGreaterThan(1);
    expect(lines[lines.length - 1]).toBe('data: [DONE]');
  });
});
