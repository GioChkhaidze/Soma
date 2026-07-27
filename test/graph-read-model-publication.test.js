import assert from 'node:assert/strict';
import test from 'node:test';

import {
  createGraphReadModelPublicationPolicy
} from '../apps/desktop/src/app/controllers/graphReadModelPublication.ts';

test('newer canvas refresh prevents an older bootstrap canvas from publishing', () => {
  const publication = createGraphReadModelPublicationPolicy('workspace-a');
  const bootstrapRead = publication.begin('canvas');
  const mutationRefresh = publication.begin('canvas');

  assert.equal(publication.canPublish(mutationRefresh), true);
  assert.equal(publication.canPublish(bootstrapRead), false);

  publication.activateWorkspace('workspace-b');
  const nextWorkspaceRead = publication.begin('canvas');

  assert.equal(publication.canPublish(mutationRefresh), false);
  assert.equal(publication.canPublish(nextWorkspaceRead), true);
});

test('newer review refresh prevents an older review read from publishing', () => {
  const publication = createGraphReadModelPublicationPolicy('workspace-a');
  const sidebarRead = publication.begin('review');
  const mutationRefresh = publication.begin('review');

  assert.equal(publication.canPublish(mutationRefresh), true);
  assert.equal(publication.canPublish(sidebarRead), false);

  publication.activateWorkspace('workspace-b');
  const nextWorkspaceRead = publication.begin('review');

  assert.equal(publication.canPublish(mutationRefresh), false);
  assert.equal(publication.canPublish(nextWorkspaceRead), true);
});

test('a layout write prevents an older bootstrap layout from publishing', () => {
  const publication = createGraphReadModelPublicationPolicy('workspace-a');
  const bootstrapRead = publication.begin('layout');

  publication.begin('layout');

  assert.equal(publication.canPublish(bootstrapRead), false);

  publication.begin('layout');
  const laterBootstrapRead = publication.begin('layout');
  const persistedWrite = publication.begin('layout');

  assert.equal(publication.canPublish(laterBootstrapRead), false);
  assert.equal(publication.canPublish(persistedWrite), true);
});
