/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import '../../../testUtilities/matchers'
import { GENESIS_BLOCK_SEQUENCE } from '../../../consensus'
import { BlockHashSerdeInstance } from '../../../serde'
import { useMinerBlockFixture } from '../../../testUtilities/fixtures'
import { createRouteTest } from '../../../testUtilities/routeTest'
import { GetBlockResponse } from './getBlock'

describe('Route chain.getBlock', () => {
  const routeTest = createRouteTest()

  it('should fail if no sequence or hash provided', async () => {
    await expect(routeTest.client.request('chain/getBlock', {}).waitForEnd()).rejects.toThrow(
      'Missing hash or sequence',
    )
  })

  it(`should fail if block can't be found with hash`, async () => {
    const hash = BlockHashSerdeInstance.serialize(Buffer.alloc(32, 'blockhashnotfound'))

    await expect(
      routeTest.client.request('chain/getBlock', { hash }).waitForEnd(),
    ).rejects.toThrow('No block found')
  })

  it(`should fail if block can't be found with sequence`, async () => {
    await expect(
      routeTest.client.request('chain/getBlock', { index: 5 }).waitForEnd(),
    ).rejects.toThrow('No block found')
  })

  it('responds with a block', async () => {
    const chain = routeTest.node.chain

    const block = await useMinerBlockFixture(chain, 2)
    await expect(chain).toAddBlock(block)

    // by hash first
    const hash = BlockHashSerdeInstance.serialize(block.header.hash)
    let response = await routeTest.client
      .request<GetBlockResponse>('chain/getBlock', { hash })
      .waitForEnd()

    expect(response.content).toMatchObject({
      timestamp: block.header.timestamp.valueOf(),
      blockIdentifier: {
        index: Number(block.header.sequence).toString(),
        hash: block.header.hash.toString('hex').toUpperCase(),
      },
      parentBlockIdentifier: {
        index: Number(GENESIS_BLOCK_SEQUENCE).toString(),
        hash: block.header.previousBlockHash.toString('hex').toUpperCase(),
      },
      metadata: {
        size: expect.any(Number),
        difficulty: Number(block.header.target.toDifficulty()),
      },
    })
    expect(response.content.transactions).toHaveLength(1)

    // now by sequence
    response = await routeTest.client
      .request<GetBlockResponse>('chain/getBlock', { index: 2 })
      .waitForEnd()
    expect(response.content.blockIdentifier.hash).toEqual(
      block.header.hash.toString('hex').toUpperCase(),
    )
  })
})
