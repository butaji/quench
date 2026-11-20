// jetstream-row: mandreel
// jetstream-files: Octane/mandreel.js
// iterations: 80
// jetstream-driver: deterministic=true
// c-sources: ../c-analogues/support/bullet/bullet-subset.cc -I../c-analogues/support/bullet/src -I../c-analogues/support/bullet/demo
/* Native analogue of JetStream's Octane/mandreel.js: the Bullet physics benchmark that Mandreel
 * translated from LLVM, built from Bullet's C++ source (support/bullet, vendored from
 * bulletphysics/bullet3 3.27, zlib license in support/bullet/LICENSE.txt).
 *
 * What the JS runs (established by running the translated code under node, see
 * tools/mandreel-native-validate.cjs):
 *   - BenchmarkDemo::initPhysics with m_benchmark == 4, so createTest4 (not createTest6: the JS
 *     never calls createTest6, and its landscape mesh is never built). createTest4 adds 15 layers of
 *     8 x 8 bodies, each a convex hull of the 43 Taru points, mass 1: 960 bodies, plus the ground box
 *     (250 x 50 x 250 at y = -50): 961 rigid bodies.
 *   - The world is the one the 3.27 source keeps commented out in initPhysics: btAxisSweep3 with a
 *     btHashedOverlappingPairCache (world AABB +-1000, 3500 handles), btCollisionDispatcher with the
 *     default collision configuration, btSequentialImpulseConstraintSolver, gravity (0, -10, 0),
 *     friction direction caching, 5 solver iterations. The JS never calls
 *     btConvexHullShape::initializePolyhedralFeatures, and this driver does not either.
 *   - 20 frames of clientMoveAndDisplay, each stepSimulation(1/60, 1, 1/60) and debugDrawWorld, as
 *     JetStream's runMandreel does. Every body falls freely: the bodies start 4.5 apart and the
 *     Taru hulls are 4 units across, so no pair overlaps and no contact forms in 20 frames.
 *   - Single precision throughout: btScalar is float (the JS keeps every Bullet float in float32).
 *
 * Each iteration rebuilds the world from scratch and destroys it at the end; the timed part is
 * that build, the 20 steps and the teardown. The JS heap reset, which copies the heap back at the
 * start of each iteration, has no counterpart here.
 *
 * Validation (after each step, outside the timer, against the node run in support/mandreel-native-expected.h):
 *   - the number of rigid bodies, overlapping pairs, persistent manifolds and contact points, at
 *     every frame of iteration 0 (the JS run has 961, 0, 0, 0 at every frame),
 *   - the sum of the body heights at every frame of iteration 0, to 1e-2,
 *   - the origin of every body after the 20th frame, in every iteration, to 1e-4 (the tolerances
 *     are explained at kPositionTolerance).
 * Prints "validation ok" or "validation FAILED" and exits 1 on a mismatch.
 */
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <vector>

#include "BulletCollision/BroadphaseCollision/btAxisSweep3.h"
#include "BulletCollision/BroadphaseCollision/btOverlappingPairCache.h"
#include "BulletCollision/CollisionDispatch/btCollisionDispatcher.h"
#include "BulletCollision/CollisionDispatch/btDefaultCollisionConfiguration.h"
#include "BulletCollision/CollisionShapes/btBoxShape.h"
#include "BulletCollision/CollisionShapes/btConvexHullShape.h"
#include "BulletCollision/NarrowPhaseCollision/btPersistentManifold.h"
#include "BulletDynamics/ConstraintSolver/btSequentialImpulseConstraintSolver.h"
#include "BulletDynamics/Dynamics/btDiscreteDynamicsWorld.h"
#include "BulletDynamics/Dynamics/btRigidBody.h"
#include "LinearMath/btDefaultMotionState.h"
#include "TaruData.h"
#include "support/mandreel-native-expected.h"

namespace {

const int kFrames = 20;
const int kLayers = 15;
const int kSize = 8;
const float kCubeSize = 1.5f;
const float kTimeStep = 1.f / 60.f;
// The JS and this build agree to within a few float ulps per body (the JS runs Bullet's older
// arithmetic, and float rounding differs in the order of operations). Observed: positions within
// 9e-6 (one ulp at 64 is 7.6e-6), height sums within 1.4e-3 over 961 bodies (about 1.4e-6 each).
// The tolerances are an order of magnitude above those.
const double kPositionTolerance = 1e-4;
const double kHeightSumTolerance = 1e-2;

struct World {
  btDefaultCollisionConfiguration *config = nullptr;
  btCollisionDispatcher *dispatcher = nullptr;
  btHashedOverlappingPairCache *pairCache = nullptr;
  btAxisSweep3 *broadphase = nullptr;
  btSequentialImpulseConstraintSolver *solver = nullptr;
  btDiscreteDynamicsWorld *dynamics = nullptr;
  btCollisionShape *groundShape = nullptr;
  btConvexHullShape *hull = nullptr;
  std::vector<btDefaultMotionState *> motionStates;
  std::vector<btRigidBody *> bodies;  // in addRigidBody order: the ground box first
};

// DemoApplication::localCreateRigidBody, as the 3.27 demo base class does it (the motion state is
// used, and the body is added to the world).
btRigidBody *addRigidBody(World &w, float mass, const btTransform &startTransform, btCollisionShape *shape) {
  btVector3 localInertia(0, 0, 0);
  if (mass != 0.f) shape->calculateLocalInertia(mass, localInertia);
  btDefaultMotionState *motionState = new btDefaultMotionState(startTransform);
  btRigidBody::btRigidBodyConstructionInfo info(mass, motionState, shape, localInertia);
  btRigidBody *body = new btRigidBody(info);
  w.dynamics->addRigidBody(body);
  w.motionStates.push_back(motionState);
  w.bodies.push_back(body);
  return body;
}

// BenchmarkDemo::initPhysics (the world, the ground box) and BenchmarkDemo::createTest4.
void buildWorld(World &w) {
  w.config = new btDefaultCollisionConfiguration();
  w.dispatcher = new btCollisionDispatcher(w.config);
  btVector3 worldAabbMin(-1000, -1000, -1000);
  btVector3 worldAabbMax(1000, 1000, 1000);
  w.pairCache = new btHashedOverlappingPairCache();
  w.broadphase = new btAxisSweep3(worldAabbMin, worldAabbMax, 3500, w.pairCache);
  w.solver = new btSequentialImpulseConstraintSolver();
  w.dynamics = new btDiscreteDynamicsWorld(w.dispatcher, w.broadphase, w.solver, w.config);
  w.dynamics->getSolverInfo().m_solverMode |= SOLVER_ENABLE_FRICTION_DIRECTION_CACHING;
  w.dynamics->getSolverInfo().m_numIterations = 5;
  w.dynamics->setGravity(btVector3(0, -10, 0));

  w.groundShape = new btBoxShape(btVector3(250., 50., 250.));
  btTransform groundTransform;
  groundTransform.setIdentity();
  groundTransform.setOrigin(btVector3(0, -50, 0));
  addRigidBody(w, 0.f, groundTransform, w.groundShape);

  // createTest4
  int size = kSize;
  float cubeSize = kCubeSize;
  float spacing = cubeSize;
  btVector3 pos(0.0f, cubeSize * 2, 0.0f);
  float offset = -size * (cubeSize * 2.0f + spacing) * 0.5f;

  w.hull = new btConvexHullShape();
  btScalar scaling(1);
  w.hull->setLocalScaling(btVector3(scaling, scaling, scaling));
  for (int i = 0; i < TaruVtxCount; i++) {
    btVector3 vtx(TaruVtx[i * 3], TaruVtx[i * 3 + 1], TaruVtx[i * 3 + 2]);
    w.hull->addPoint(vtx * btScalar(1. / scaling));
  }
  btTransform trans;
  trans.setIdentity();
  float mass = 1.f;
  btVector3 localInertia(0, 0, 0);
  w.hull->calculateLocalInertia(mass, localInertia);

  for (int k = 0; k < kLayers; k++) {
    for (int j = 0; j < size; j++) {
      pos[2] = offset + (float)j * (cubeSize * 2.0f + spacing);
      for (int i = 0; i < size; i++) {
        pos[0] = offset + (float)i * (cubeSize * 2.0f + spacing);
        trans.setOrigin(pos);
        addRigidBody(w, mass, trans, w.hull);
      }
    }
    offset -= 0.05f * spacing * (size - 1);
    spacing *= 1.01f;
    pos[1] += (cubeSize * 2.0f + spacing);
  }
}

void destroyWorld(World &w) {
  for (btRigidBody *body : w.bodies) {
    w.dynamics->removeRigidBody(body);
    delete body;
  }
  for (btDefaultMotionState *motionState : w.motionStates) delete motionState;
  delete w.hull;
  delete w.groundShape;
  delete w.dynamics;
  delete w.solver;
  delete w.broadphase;
  delete w.pairCache;
  delete w.dispatcher;
  delete w.config;
  w = World();
}

struct Counts {
  int bodies;
  int pairs;
  int manifolds;
  int contacts;
};

Counts countsOf(const World &w) {
  Counts c;
  c.bodies = w.dynamics->getNumCollisionObjects();
  c.pairs = w.broadphase->getOverlappingPairCache()->getNumOverlappingPairs();
  c.manifolds = w.dispatcher->getNumManifolds();
  c.contacts = 0;
  for (int i = 0; i < c.manifolds; i++) c.contacts += w.dispatcher->getManifoldByIndexInternal(i)->getNumContacts();
  return c;
}

double heightSum(const World &w) {
  double sum = 0;
  for (const btRigidBody *body : w.bodies) sum += body->getWorldTransform().getOrigin()[1];
  return sum;
}

double now_ms() {
  using clock = std::chrono::steady_clock;
  return std::chrono::duration<double, std::milli>(clock::now().time_since_epoch()).count();
}

}  // namespace

int main(int argc, char **argv) {
  int iterations = argc > 1 ? std::atoi(argv[1]) : 80;
  /* argv[2] is the JetStream root: nothing is read from it. */
  bool ok = true;
  double maxPositionError = 0;
  double maxHeightSumError = 0;
  double total_ms = 0;

  for (int it = 0; it < iterations; it++) {
    World w;
    double elapsed = 0;

    double t0 = now_ms();
    buildWorld(w);
    elapsed += now_ms() - t0;

    for (int frame = 0; frame < kFrames; frame++) {
      t0 = now_ms();
      w.dynamics->stepSimulation(kTimeStep, 1, kTimeStep);
      w.dynamics->debugDrawWorld();
      elapsed += now_ms() - t0;

      if (it == 0) {
        Counts c = countsOf(w);
        if (c.bodies != kExpectBodies || c.pairs != kExpectPairs[frame] || c.manifolds != kExpectManifolds[frame] ||
            c.contacts != kExpectContacts[frame]) {
          std::fprintf(stderr, "frame %d: bodies %d pairs %d manifolds %d contacts %d; expected %d %d %d %d\n", frame + 1, c.bodies,
                       c.pairs, c.manifolds, c.contacts, kExpectBodies, kExpectPairs[frame], kExpectManifolds[frame],
                       kExpectContacts[frame]);
          ok = false;
        }
        double error = std::fabs(heightSum(w) - kExpectHeightSum[frame]);
        if (error > maxHeightSumError) maxHeightSumError = error;
        if (error > kHeightSumTolerance) {
          std::fprintf(stderr, "frame %d: height sum %.9g; expected %.9g\n", frame + 1, heightSum(w), kExpectHeightSum[frame]);
          ok = false;
        }
      }
    }

    // The positions after the last frame, body by body, in the JS's order.
    for (int b = 0; b < kExpectBodies; b++) {
      btVector3 origin = w.bodies[b]->getWorldTransform().getOrigin();
      for (int axis = 0; axis < 3; axis++) {
        double error = std::fabs(double(origin[axis]) - double(kExpectFinal[3 * b + axis]));
        if (error > maxPositionError) maxPositionError = error;
        if (error > kPositionTolerance) ok = false;
      }
    }

    t0 = now_ms();
    destroyWorld(w);
    elapsed += now_ms() - t0;
    total_ms += elapsed;
  }

  std::printf("mandreel %d iterations %.3f ms\n", iterations, total_ms);
  if (!ok) std::fprintf(stderr, "validation: a count, a height sum or a position differs from node\n");
  std::printf("max position error %.3g, max height sum error %.3g\n", maxPositionError, maxHeightSumError);
  std::printf("validation %s\n", ok ? "ok" : "FAILED");
  return ok ? 0 : 1;
}
