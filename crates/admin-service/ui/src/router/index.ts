import { createRouter, createWebHashHistory } from 'vue-router';
import OverviewView from '../views/OverviewView.vue';
import ClusterView from '../views/ClusterView.vue';
import ServicesView from '../views/ServicesView.vue';
import StoreView from '../views/StoreView.vue';
import ConfigView from '../views/ConfigView.vue';
import EnvView from '../views/EnvView.vue';

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: '/',
      name: 'overview',
      component: OverviewView,
      meta: { layout: 'default' },
    },
    {
      path: '/cluster',
      name: 'cluster',
      component: ClusterView,
      meta: { layout: 'full' },
    },
    {
      path: '/control-plane',
      redirect: '/cluster',
    },
    {
      path: '/config',
      name: 'config',
      component: ConfigView,
      meta: { layout: 'default' },
    },
    {
      path: '/services',
      name: 'services',
      component: ServicesView,
      meta: { layout: 'default' },
    },
    {
      path: '/store',
      name: 'store',
      component: StoreView,
      meta: { layout: 'default' },
    },
    {
      path: '/logs',
      redirect: '/config',
    },
    {
      path: '/events',
      redirect: '/',
    },
    {
      path: '/env',
      name: 'env',
      component: EnvView,
      meta: { layout: 'default' },
    },
    {
      path: '/:pathMatch(.*)*',
      redirect: '/',
    },
  ],
});

export default router;
