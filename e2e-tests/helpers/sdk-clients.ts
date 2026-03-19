import OpenAI from 'openai';
import Anthropic from '@anthropic-ai/sdk';

const GATEWAY_URL = process.env.GATEWAY_URL || 'http://localhost:9999';
const API_KEY = process.env.API_KEY || '';
const DEFAULT_MODEL = process.env.DEFAULT_MODEL || 'claude-sonnet-4-20250514';

export function createOpenAIClient(): OpenAI {
  return new OpenAI({ baseURL: `${GATEWAY_URL}/v1`, apiKey: API_KEY });
}

export function createAnthropicClient(): Anthropic {
  return new Anthropic({ baseURL: GATEWAY_URL, apiKey: API_KEY });
}

export { DEFAULT_MODEL, GATEWAY_URL, API_KEY };
