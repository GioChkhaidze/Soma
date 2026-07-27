import assert from 'node:assert/strict';
import test from 'node:test';

import { createGraphExtractionJobResultSchema, jobRunSchema } from '../packages/contracts/src/schemas.ts';
import { compileCoverageNotice } from '../apps/desktop/src/features/job-runs/jobRunsViewModel.ts';

const coverage = {
  chunkCount: 500,
  includedChunkCount: 500,
  totalChunkCount: 501,
  truncated: true
};

test('compile job contracts preserve partial chunk coverage', () => {
  const files = {
    metadata: 'metadata.json',
    instructions: 'instructions.md',
    runtime: 'runtime.json',
    chunks: 'chunks.json',
    currentGraphSnapshot: 'current_graph_snapshot.json',
    graphPatchSchema: 'graph_patch.schema.json',
    outputPatch: 'output_patch.json'
  };
  const created = createGraphExtractionJobResultSchema.parse({
    jobId: 'job_coverage',
    jobDir: 'jobs/job_coverage',
    files,
    ...coverage
  });
  const listed = jobRunSchema.parse({
    jobId: 'job_coverage',
    jobDir: 'jobs/job_coverage',
    jobKind: 'graph_extraction',
    createdAt: null,
    schemaVersion: 1,
    sourceCount: 1,
    files: { metadata: files.metadata },
    metadataExists: true,
    outputPatchExists: true,
    ...coverage
  });

  assert.deepEqual(
    [created.includedChunkCount, created.totalChunkCount, created.truncated],
    [500, 501, true]
  );
  assert.deepEqual(
    [listed.includedChunkCount, listed.totalChunkCount, listed.truncated],
    [500, 501, true]
  );
});

test('compile coverage notice appears only for a partial run', () => {
  assert.equal(
    compileCoverageNotice(coverage),
    'Used 500 of 501 source chunks; later chunks were not included in this run.'
  );
  assert.equal(compileCoverageNotice({ chunkCount: 12, totalChunkCount: 12, truncated: false }), null);
});
