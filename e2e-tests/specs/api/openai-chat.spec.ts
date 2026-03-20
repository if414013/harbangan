import { test, expect } from '@playwright/test';

test.describe('OpenAI chat completions (non-streaming)', () => {
  test.skip(!process.env.API_KEY, 'Requires API_KEY environment variable');

  test('POST /v1/chat/completions returns valid response', async ({ request }) => {
    const response = await request.post('/v1/chat/completions', {
      data: {
        model: 'claude-sonnet-4-20250514',
        messages: [{ role: 'user', content: 'Say "hello" and nothing else.' }],
        max_tokens: 32,
      },
    });
    expect(response.status()).toBe(200);

    const body = await response.json();
    expect(body).toHaveProperty('choices');
    expect(Array.isArray(body.choices)).toBe(true);
    expect(body.choices.length).toBeGreaterThan(0);
    expect(body.choices[0]).toHaveProperty('message');
    expect(body.choices[0].message).toHaveProperty('content');
    expect(body.choices[0].message).toHaveProperty('role', 'assistant');
    expect(body).toHaveProperty('usage');
    expect(body.usage).toHaveProperty('prompt_tokens');
    expect(body.usage).toHaveProperty('completion_tokens');
  });
});
