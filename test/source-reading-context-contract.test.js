import assert from 'node:assert/strict';
import test from 'node:test';

import { sourceReadingContextSchema } from '../packages/contracts/src/schemas.ts';
import fixture from './source-reading-context-cases.json' with { type: 'json' };

test('source reading context matches shared canonical cases', () => {
  for (const testCase of fixture.cases) {
    const parsed = sourceReadingContextSchema.safeParse(testCase.input);
    assert.equal(parsed.success, testCase.canonical !== null, testCase.name);
    if (testCase.canonical === null) continue;
    assert.deepEqual(parsed.data, testCase.canonical, testCase.name);
  }
});

test('source reading context truncates bounded strings by Unicode character', () => {
  const { bounds, truncation_case: testCase } = fixture;
  const input = { ...testCase.input };
  const expected = { ...testCase.input };
  for (const [field, maxCharacters] of Object.entries(bounds)) {
    input[field] = testCase.character.repeat(maxCharacters + 1);
    expected[field] = testCase.character.repeat(maxCharacters);
  }

  assert.deepEqual(sourceReadingContextSchema.parse(input), expected, testCase.name);
});
