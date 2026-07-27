import assert from 'node:assert/strict';
import test from 'node:test';

import { createSearchRequestOwnership } from '../apps/desktop/src/features/search/searchRequestOwnership.ts';
import {
  graphCanvasNodesSchema,
  graphNodeSearchArgsSchema
} from '../packages/contracts/src/schemas.ts';

test('global graph search validates its typed command contracts', () => {
  assert.deepEqual(graphNodeSearchArgsSchema.parse({ query: '  tail  ', limit: 5 }), {
    query: 'tail',
    limit: 5
  });
  assert.deepEqual(
    graphCanvasNodesSchema.parse([nodeCard('node-tail', 'Tail node')]),
    [nodeCard('node-tail', 'Tail node')]
  );
  assert.throws(() => graphNodeSearchArgsSchema.parse({ query: 'tail', limit: 21 }));
});

test('a stale graph search response cannot replace the latest result', async () => {
  const ownership = createSearchRequestOwnership();
  let resolveSlow;
  const slowResponse = new Promise((resolve) => {
    resolveSlow = resolve;
  });
  let visibleResults = [];
  const publish = async (request, response) => {
    const results = await response;
    if (ownership.owns(request)) visibleResults = results;
  };

  const slowRequest = ownership.begin();
  const slowPublication = publish(slowRequest, slowResponse);
  const fastRequest = ownership.begin();
  await publish(fastRequest, Promise.resolve(['fast result']));
  resolveSlow(['stale result']);
  await slowPublication;

  assert.deepEqual(visibleResults, ['fast result']);
});

function nodeCard(id, title) {
  return {
    id,
    type: 'concept',
    title,
    preview: `${title} preview`,
    source_chunk_ids: [],
    body_version: 1,
    status: 'active',
    markers: []
  };
}
