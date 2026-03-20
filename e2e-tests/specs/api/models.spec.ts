import { test, expect } from '@playwright/test';

test.describe('Models endpoint', () => {
  test.skip(!process.env.API_KEY, 'Requires API_KEY environment variable');

  test('GET /v1/models returns model list', async ({ request }) => {
    const response = await request.get('/v1/models');
    expect(response.status()).toBe(200);
    const body = await response.json();
    expect(body).toHaveProperty('data');
    expect(Array.isArray(body.data)).toBe(true);
    expect(body.data.length).toBeGreaterThan(0);

    const model = body.data[0];
    expect(model).toHaveProperty('id');
    expect(model).toHaveProperty('object');
  });
});
